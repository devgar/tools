//! Persistence of the `/sync` `next_batch` token between restarts.

use std::{ffi::OsString, path::PathBuf};

use anyhow::Result;
use tracing::warn;

use crate::windmill::Windmill;

pub const VAR_SINCE: &str = "f/matrix_bot/since_token";

pub enum SinceStore<'a> {
    File(PathBuf),
    Windmill(&'a Windmill),
}

impl SinceStore<'_> {
    /// Priority: stored token > `seed` > nothing (initial sync).
    pub async fn load(&self, seed: Option<String>) -> Option<String> {
        let stored = match self {
            Self::File(path) => match tokio::fs::read_to_string(path).await {
                Ok(s) => Some(s),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => {
                    warn!(path = %path.display(), "could not read since file: {e}");
                    None
                }
            },
            Self::Windmill(wm) => wm
                .get_variable(VAR_SINCE)
                .await
                .inspect_err(|e| warn!("could not read {VAR_SINCE}: {e:#}"))
                .ok(),
        };
        stored
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or(seed)
    }

    pub async fn save(&self, token: &str) -> Result<()> {
        match self {
            Self::File(path) => {
                // Atomic write: temp file in the same directory + rename, so a
                // crash never leaves a half-written token.
                let mut tmp = OsString::from(path.as_os_str());
                tmp.push(".tmp");
                let tmp = PathBuf::from(tmp);
                tokio::fs::write(&tmp, token).await?;
                tokio::fs::rename(&tmp, path).await?;
                Ok(())
            }
            Self::Windmill(wm) => wm.set_variable(VAR_SINCE, token).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!("matrix-listener-{}", uuid::Uuid::new_v4()))
    }

    fn seed() -> Option<String> {
        Some("seed".into())
    }

    #[tokio::test]
    async fn file_wins_over_seed() {
        let path = temp_path();
        std::fs::write(&path, "stored\n").unwrap();
        let store = SinceStore::File(path.clone());
        assert_eq!(store.load(seed()).await.as_deref(), Some("stored"));
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn seed_used_when_file_missing() {
        let store = SinceStore::File(temp_path());
        assert_eq!(store.load(seed()).await.as_deref(), Some("seed"));
    }

    #[tokio::test]
    async fn seed_used_when_file_blank() {
        let path = temp_path();
        std::fs::write(&path, "  \n").unwrap();
        let store = SinceStore::File(path.clone());
        assert_eq!(store.load(seed()).await.as_deref(), Some("seed"));
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn nothing_when_no_file_and_no_seed() {
        let store = SinceStore::File(temp_path());
        assert_eq!(store.load(None).await, None);
    }

    #[tokio::test]
    async fn save_round_trips_and_leaves_no_temp_file() {
        let dir = temp_path();
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("since.token");
        let store = SinceStore::File(path.clone());

        store.save("first").await.unwrap();
        store.save("second").await.unwrap();

        assert_eq!(store.load(seed()).await.as_deref(), Some("second"));
        let entries: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(entries.len(), 1, "temp file left behind");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
