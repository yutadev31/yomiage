use std::{
    collections::{HashMap, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode};
use sha2::{Digest, Sha256};

static CACHE_WRITE_COUNTER: AtomicU64 = AtomicU64::new(0);

const DEFAULT_BASE_URL: &str = "http://127.0.0.1:50021";
const DEFAULT_SPEAKER: u32 = 3;
const DEFAULT_CACHE_ENTRIES: usize = 128;
const KIBIBYTE: usize = 1024;
const MEBIBYTE: usize = KIBIBYTE * KIBIBYTE;
const GIBIBYTE: usize = KIBIBYTE * MEBIBYTE;
const DEFAULT_CACHE_MAX_BYTES: usize = 64 * MEBIBYTE;
const DEFAULT_DISK_CACHE_DIR: &str = ".voicevox-cache";
const DEFAULT_DISK_CACHE_ENTRIES: usize = 10_000;
const DEFAULT_DISK_CACHE_MAX_BYTES: usize = GIBIBYTE;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const CACHE_FILE_EXTENSION: &str = "wav";
const CACHE_KEY_SEPARATOR: u8 = 0;

/// Client for a locally running VOICEVOX Engine.
#[derive(Clone)]
pub struct Voicevox {
    client: Client,
    base_url: String,
    speaker: u32,
    cache: Arc<Mutex<AudioCache>>,
    disk_cache: DiskCache,
}

impl Voicevox {
    /// Creates a client using `VOICEVOX_URL` and `VOICEVOX_SPEAKER`.
    ///
    /// The defaults are `http://127.0.0.1:50021` and speaker ID `3`.
    pub fn from_env() -> Result<Self> {
        let base_url =
            std::env::var("VOICEVOX_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_owned());
        let speaker = std::env::var("VOICEVOX_SPEAKER")
            .ok()
            .map(|value| value.parse())
            .transpose()
            .context("VOICEVOX_SPEAKER must be an unsigned integer")?
            .unwrap_or(DEFAULT_SPEAKER);
        let cache_entries = env_usize("VOICEVOX_CACHE_ENTRIES", DEFAULT_CACHE_ENTRIES)?;
        let cache_max_bytes = env_usize("VOICEVOX_CACHE_MAX_BYTES", DEFAULT_CACHE_MAX_BYTES)?;
        let disk_cache_dir = std::env::var("VOICEVOX_DISK_CACHE_DIR")
            .unwrap_or_else(|_| DEFAULT_DISK_CACHE_DIR.to_owned());
        if disk_cache_dir.trim().is_empty() {
            bail!("VOICEVOX_DISK_CACHE_DIR must not be empty");
        }
        let disk_cache_entries =
            env_usize("VOICEVOX_DISK_CACHE_ENTRIES", DEFAULT_DISK_CACHE_ENTRIES)?;
        let disk_cache_max_bytes = env_usize(
            "VOICEVOX_DISK_CACHE_MAX_BYTES",
            DEFAULT_DISK_CACHE_MAX_BYTES,
        )?;

        Self::new_with_cache(
            base_url,
            speaker,
            cache_entries,
            cache_max_bytes,
            PathBuf::from(disk_cache_dir),
            disk_cache_entries,
            disk_cache_max_bytes,
        )
    }

    pub fn new(base_url: impl Into<String>, speaker: u32) -> Result<Self> {
        Self::new_with_cache(
            base_url,
            speaker,
            DEFAULT_CACHE_ENTRIES,
            DEFAULT_CACHE_MAX_BYTES,
            PathBuf::from(DEFAULT_DISK_CACHE_DIR),
            DEFAULT_DISK_CACHE_ENTRIES,
            DEFAULT_DISK_CACHE_MAX_BYTES,
        )
    }

    fn new_with_cache(
        base_url: impl Into<String>,
        speaker: u32,
        cache_entries: usize,
        cache_max_bytes: usize,
        disk_cache_dir: PathBuf,
        disk_cache_entries: usize,
        disk_cache_max_bytes: usize,
    ) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        if base_url.is_empty() {
            bail!("VOICEVOX URL must not be empty");
        }

        let client = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("failed to create VOICEVOX HTTP client")?;

        Ok(Self {
            client,
            base_url,
            speaker,
            cache: Arc::new(Mutex::new(AudioCache::new(cache_entries, cache_max_bytes))),
            disk_cache: DiskCache::new(disk_cache_dir, disk_cache_entries, disk_cache_max_bytes),
        })
    }

    /// Converts text to a WAV file through VOICEVOX Engine.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
        if let Some(wav) = self
            .cache
            .lock()
            .expect("audio cache lock poisoned")
            .get(text)
        {
            return Ok(wav);
        }

        let cache_key = self.cache_key(text);
        match self.disk_cache.get(&cache_key).await {
            Ok(Some(wav)) => {
                self.cache
                    .lock()
                    .expect("audio cache lock poisoned")
                    .insert(text, &wav);
                return Ok(wav);
            }
            Ok(None) => {}
            Err(err) => eprintln!("Failed to read VOICEVOX disk cache: {err:#}"),
        }

        let wav = self.synthesize_uncached(text).await?;
        self.cache
            .lock()
            .expect("audio cache lock poisoned")
            .insert(text, &wav);
        if let Err(err) = self.disk_cache.insert(&cache_key, &wav).await {
            eprintln!("Failed to write VOICEVOX disk cache: {err:#}");
        }
        Ok(wav)
    }

    fn cache_key(&self, text: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.base_url.as_bytes());
        hasher.update([CACHE_KEY_SEPARATOR]);
        hasher.update(self.speaker.to_le_bytes());
        hasher.update([CACHE_KEY_SEPARATOR]);
        hasher.update(text.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    async fn synthesize_uncached(&self, text: &str) -> Result<Vec<u8>> {
        let query_url = format!("{}/audio_query", self.base_url);
        let audio_query = self
            .client
            .post(query_url)
            .query(&[("text", text), ("speaker", &self.speaker.to_string())])
            .send()
            .await
            .context("VOICEVOX audio_query request failed")?;
        let audio_query = response_bytes(audio_query, "audio_query").await?;

        let synthesis_url = format!("{}/synthesis", self.base_url);
        let wav = self
            .client
            .post(synthesis_url)
            .query(&[("speaker", self.speaker)])
            .header("content-type", "application/json")
            .body(audio_query)
            .send()
            .await
            .context("VOICEVOX synthesis request failed")?;
        response_bytes(wav, "synthesis").await
    }

    /// Ensures that the configured VOICEVOX Engine is reachable.
    pub async fn check_connection(&self) -> Result<()> {
        let response = self
            .client
            .get(format!("{}/version", self.base_url))
            .send()
            .await
            .context("VOICEVOX Engine is not reachable")?;
        response_bytes(response, "version").await?;
        Ok(())
    }
}

/// Persistent WAV cache shared across process restarts.
#[derive(Clone)]
struct DiskCache {
    directory: Arc<PathBuf>,
    max_entries: usize,
    max_bytes: usize,
}

impl DiskCache {
    fn new(directory: PathBuf, max_entries: usize, max_bytes: usize) -> Self {
        Self {
            directory: Arc::new(directory),
            max_entries,
            max_bytes,
        }
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>> {
        if self.max_entries == 0 || self.max_bytes == 0 {
            return Ok(None);
        }

        let path = self.path_for(key);
        match tokio::fs::read(&path).await {
            Ok(wav) if !wav.is_empty() => Ok(Some(wav)),
            Ok(_) => {
                let _ = tokio::fs::remove_file(path).await;
                Ok(None)
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
        }
    }

    async fn insert(&self, key: &str, wav: &[u8]) -> Result<()> {
        if self.max_entries == 0 || self.max_bytes == 0 || wav.len() > self.max_bytes {
            return Ok(());
        }

        tokio::fs::create_dir_all(self.directory.as_ref())
            .await
            .with_context(|| format!("failed to create {}", self.directory.display()))?;

        let destination = self.path_for(key);
        let write_id = CACHE_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary =
            destination.with_extension(format!("{}.{}.tmp", std::process::id(), write_id));
        tokio::fs::write(&temporary, wav)
            .await
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        tokio::fs::rename(&temporary, &destination)
            .await
            .with_context(|| format!("failed to store {}", destination.display()))?;
        self.trim(&destination).await
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.directory.join(format!("{key}.{CACHE_FILE_EXTENSION}"))
    }

    async fn trim(&self, keep: &Path) -> Result<()> {
        let mut files = Vec::new();
        let mut total_bytes = 0;
        let mut total_entries = 0;
        let mut directory = tokio::fs::read_dir(self.directory.as_ref())
            .await
            .with_context(|| format!("failed to read {}", self.directory.display()))?;

        while let Some(entry) = directory.next_entry().await? {
            let path = entry.path();
            if path.extension().and_then(|extension| extension.to_str())
                != Some(CACHE_FILE_EXTENSION)
            {
                continue;
            }

            let metadata = entry.metadata().await?;
            total_entries += 1;
            total_bytes += metadata.len() as usize;
            if path != keep {
                files.push((
                    metadata
                        .modified()
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
                    path,
                    metadata.len() as usize,
                ));
            }
        }

        files.sort_by_key(|(modified, _, _)| *modified);
        for (_, path, size) in files {
            if total_entries <= self.max_entries && total_bytes <= self.max_bytes {
                break;
            }
            tokio::fs::remove_file(&path).await.with_context(|| {
                format!("failed to remove expired cache file {}", path.display())
            })?;
            total_entries -= 1;
            total_bytes -= size;
        }
        Ok(())
    }
}

fn env_usize(name: &str, default: usize) -> Result<usize> {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .parse()
                .with_context(|| format!("{name} must be an unsigned integer"))
        })
        .transpose()
        .map(|value| value.unwrap_or(default))
}

/// A byte-bounded least-recently-used cache of synthesized WAV files.
struct AudioCache {
    entries: HashMap<String, Vec<u8>>,
    usage_order: VecDeque<String>,
    max_entries: usize,
    max_bytes: usize,
    total_bytes: usize,
}

impl AudioCache {
    fn new(max_entries: usize, max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            usage_order: VecDeque::new(),
            max_entries,
            max_bytes,
            total_bytes: 0,
        }
    }

    fn get(&mut self, text: &str) -> Option<Vec<u8>> {
        let wav = self.entries.get(text)?.clone();
        self.touch(text);
        Some(wav)
    }

    fn insert(&mut self, text: &str, wav: &[u8]) {
        if self.max_entries == 0 || self.max_bytes == 0 || wav.len() > self.max_bytes {
            return;
        }

        if let Some(previous) = self.entries.insert(text.to_owned(), wav.to_vec()) {
            self.total_bytes -= previous.len();
        }
        self.total_bytes += wav.len();
        self.touch(text);

        while self.entries.len() > self.max_entries || self.total_bytes > self.max_bytes {
            let Some(oldest) = self.usage_order.pop_front() else {
                break;
            };
            if let Some(wav) = self.entries.remove(&oldest) {
                self.total_bytes -= wav.len();
            }
        }
    }

    fn touch(&mut self, text: &str) {
        self.usage_order.retain(|key| key != text);
        self.usage_order.push_back(text.to_owned());
    }
}

async fn response_bytes(response: reqwest::Response, endpoint: &str) -> Result<Vec<u8>> {
    let status = response.status();
    if status.is_success() {
        return response
            .bytes()
            .await
            .map(|bytes| bytes.to_vec())
            .with_context(|| format!("failed to read VOICEVOX {endpoint} response"));
    }

    let message = response.text().await.unwrap_or_default();
    if status == StatusCode::NOT_FOUND {
        bail!("VOICEVOX {endpoint} endpoint was not found; is VOICEVOX Engine running?");
    }
    bail!("VOICEVOX {endpoint} failed with {status}: {message}");
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    use super::{AudioCache, DiskCache};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn test_cache_dir() -> PathBuf {
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("yomiage-disk-cache-{}-{id}", std::process::id()))
    }

    #[test]
    fn returns_cached_audio_and_promotes_it_to_most_recent() {
        let mut cache = AudioCache::new(2, 100);
        cache.insert("first", &[1]);
        cache.insert("second", &[2]);
        assert_eq!(cache.get("first"), Some(vec![1]));

        cache.insert("third", &[3]);
        assert_eq!(cache.get("first"), Some(vec![1]));
        assert_eq!(cache.get("second"), None);
        assert_eq!(cache.get("third"), Some(vec![3]));
    }

    #[test]
    fn evicts_entries_when_the_byte_limit_is_exceeded() {
        let mut cache = AudioCache::new(10, 3);
        cache.insert("first", &[1, 2]);
        cache.insert("second", &[3, 4]);

        assert_eq!(cache.get("first"), None);
        assert_eq!(cache.get("second"), Some(vec![3, 4]));
    }

    #[test]
    fn skips_entries_larger_than_the_cache_limit() {
        let mut cache = AudioCache::new(10, 2);
        cache.insert("large", &[1, 2, 3]);

        assert_eq!(cache.get("large"), None);
    }

    #[tokio::test]
    async fn disk_cache_persists_audio_and_enforces_its_size_limit() {
        let directory = test_cache_dir();
        let cache = DiskCache::new(directory.clone(), 10, 3);
        cache.insert("first", &[1, 2]).await.unwrap();
        assert_eq!(cache.get("first").await.unwrap(), Some(vec![1, 2]));

        cache.insert("second", &[3, 4]).await.unwrap();
        assert_eq!(cache.get("first").await.unwrap(), None);
        assert_eq!(cache.get("second").await.unwrap(), Some(vec![3, 4]));

        tokio::fs::remove_dir_all(directory).await.unwrap();
    }

    #[tokio::test]
    async fn disk_cache_enforces_its_entry_limit() {
        let directory = test_cache_dir();
        let cache = DiskCache::new(directory.clone(), 2, 100);
        cache.insert("first", &[1]).await.unwrap();
        cache.insert("second", &[2]).await.unwrap();
        cache.insert("third", &[3]).await.unwrap();

        assert_eq!(cache.get("third").await.unwrap(), Some(vec![3]));
        let mut directory_entries = tokio::fs::read_dir(&directory).await.unwrap();
        let mut cache_entries = 0;
        while directory_entries.next_entry().await.unwrap().is_some() {
            cache_entries += 1;
        }
        assert_eq!(cache_entries, 2);

        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
}
