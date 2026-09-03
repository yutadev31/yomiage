use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode};

/// Client for a locally running VOICEVOX Engine.
#[derive(Clone)]
pub struct Voicevox {
    client: Client,
    base_url: String,
    speaker: u32,
    cache: Arc<Mutex<AudioCache>>,
}

impl Voicevox {
    /// Creates a client using `VOICEVOX_URL` and `VOICEVOX_SPEAKER`.
    ///
    /// The defaults are `http://127.0.0.1:50021` and speaker ID `3`.
    pub fn from_env() -> Result<Self> {
        let base_url =
            std::env::var("VOICEVOX_URL").unwrap_or_else(|_| "http://127.0.0.1:50021".to_owned());
        let speaker = std::env::var("VOICEVOX_SPEAKER")
            .ok()
            .map(|value| value.parse())
            .transpose()
            .context("VOICEVOX_SPEAKER must be an unsigned integer")?
            .unwrap_or(3);
        let cache_entries = env_usize("VOICEVOX_CACHE_ENTRIES", 128)?;
        let cache_max_bytes = env_usize("VOICEVOX_CACHE_MAX_BYTES", 64 * 1024 * 1024)?;

        Self::new_with_cache(base_url, speaker, cache_entries, cache_max_bytes)
    }

    pub fn new(base_url: impl Into<String>, speaker: u32) -> Result<Self> {
        Self::new_with_cache(base_url, speaker, 128, 64 * 1024 * 1024)
    }

    fn new_with_cache(
        base_url: impl Into<String>,
        speaker: u32,
        cache_entries: usize,
        cache_max_bytes: usize,
    ) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        if base_url.is_empty() {
            bail!("VOICEVOX URL must not be empty");
        }

        let client = Client::builder()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(30))
            .build()
            .context("failed to create VOICEVOX HTTP client")?;

        Ok(Self {
            client,
            base_url,
            speaker,
            cache: Arc::new(Mutex::new(AudioCache::new(cache_entries, cache_max_bytes))),
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

        let wav = self.synthesize_uncached(text).await?;
        self.cache
            .lock()
            .expect("audio cache lock poisoned")
            .insert(text, &wav);
        Ok(wav)
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
    use super::AudioCache;

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
}
