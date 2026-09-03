use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode};

/// Client for a locally running VOICEVOX Engine.
#[derive(Clone)]
pub struct Voicevox {
    client: Client,
    base_url: String,
    speaker: u32,
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

        Self::new(base_url, speaker)
    }

    pub fn new(base_url: impl Into<String>, speaker: u32) -> Result<Self> {
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
        })
    }

    /// Converts text to a WAV file through VOICEVOX Engine.
    pub async fn synthesize(&self, text: &str) -> Result<Vec<u8>> {
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
