//! Persistent per-user client preferences stored under `~/.tellit/config.json`.

extern crate alloc;

use alloc::string::String;
use core::sync::atomic::{AtomicU64, Ordering};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::task::spawn_blocking;

/// Maximum serialized configuration size accepted and written to disk (64 KiB).
pub const MAX_USER_CONFIG_BYTES: usize = 64 * 1024;

const CONFIG_VERSION: u32 = 1;
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Serialized user configuration document persisted at `~/.tellit/config.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserConfig {
    /// Schema version (always `1`).
    pub version: u32,
    /// Client-managed settings dictionary shallow-merged on `PUT /api/user-settings`.
    pub client: Map<String, Value>,
}

impl Default for UserConfig {
    fn default() -> Self {
        Self {
            version: CONFIG_VERSION,
            client: Map::new(),
        }
    }
}

/// Errors returned when validating, reading, or updating user configuration files.
#[derive(Debug, Error)]
pub enum UserConfigError {
    /// Request body lacked a JSON object `client` field.
    #[error("{0}")]
    InvalidPatch(String),
    /// Patch or merged configuration exceeded [`MAX_USER_CONFIG_BYTES`].
    #[error("user settings exceed the maximum allowed size of {MAX_USER_CONFIG_BYTES} bytes")]
    TooLarge,
    /// Filesystem I/O or background blocking-task error.
    #[error("user settings I/O failed: {0}")]
    Io(#[from] io::Error),
    /// JSON serialization error when formatting the config document.
    #[error("failed to serialize user settings: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Resolves the default `~/.tellit/config.json` path (honoring `TELLIT_CONFIG_DIR`).
#[must_use]
pub fn default_user_config_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("TELLIT_CONFIG_DIR")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
    {
        return dir.join("config.json");
    }

    if let Some(home) = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
    {
        return home.join(".tellit").join("config.json");
    }

    PathBuf::from(".tellit").join("config.json")
}

/// Synchronized handle reading and atomically updating a JSON user configuration file.
pub struct UserConfigStore {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl UserConfigStore {
    /// Creates a store backed by `path` (or [`default_user_config_path`] when `None`).
    #[must_use]
    pub fn new(path: Option<PathBuf>) -> Self {
        Self {
            path: path.unwrap_or_else(default_user_config_path),
            write_lock: Mutex::new(()),
        }
    }

    /// Reads the stored user configuration, falling back to empty defaults when missing or corrupt.
    pub async fn read(&self) -> UserConfig {
        let path = self.path.clone();
        spawn_blocking(move || read_user_config_sync(&path))
            .await
            .unwrap_or_default()
    }

    /// Validates `body` (`{ "client": { ... } }`), shallow-merges keys into disk state, and persists atomically.
    ///
    /// # Errors
    ///
    /// Returns [`UserConfigError`] when the payload shape is invalid, exceeds 64 KiB, or cannot be written.
    pub async fn update_client(&self, body: Value) -> Result<UserConfig, UserConfigError> {
        let patch = parse_user_settings_patch(body)?;
        let _guard = self.write_lock.lock().await;
        let path = self.path.clone();
        spawn_blocking(move || update_user_config_sync(&path, patch))
            .await
            .map_err(|err| UserConfigError::Io(io::Error::other(err)))?
    }
}

fn parse_user_settings_patch(body: Value) -> Result<Map<String, Value>, UserConfigError> {
    let Value::Object(mut root) = body else {
        return Err(UserConfigError::InvalidPatch(String::from(
            "expected a JSON object body with a client object",
        )));
    };
    let Some(Value::Object(client_patch)) = root.remove("client") else {
        return Err(UserConfigError::InvalidPatch(String::from(
            "request body must contain a JSON object \"client\" field",
        )));
    };
    let patch_bytes = serde_json::to_vec(&client_patch)?;
    if patch_bytes.len() > MAX_USER_CONFIG_BYTES {
        return Err(UserConfigError::TooLarge);
    }
    Ok(client_patch)
}

fn read_user_config_sync(path: &Path) -> UserConfig {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return UserConfig::default();
    };
    let Ok(Value::Object(mut obj)) = serde_json::from_str::<Value>(&raw) else {
        return UserConfig::default();
    };
    let Some(Value::Object(client)) = obj.remove("client") else {
        return UserConfig::default();
    };
    UserConfig {
        version: CONFIG_VERSION,
        client,
    }
}

fn update_user_config_sync(
    path: &Path,
    patch: Map<String, Value>,
) -> Result<UserConfig, UserConfigError> {
    let mut current = read_user_config_sync(path);
    for (key, value) in patch {
        current.client.insert(key, value);
    }
    current.version = CONFIG_VERSION;

    let mut serialized = serde_json::to_string_pretty(&current)?;
    serialized.push('\n');
    if serialized.len() > MAX_USER_CONFIG_BYTES {
        return Err(UserConfigError::TooLarge);
    }

    let parent = path.parent().filter(|p| !p.as_os_str().is_empty());
    if let Some(dir) = parent {
        std::fs::create_dir_all(dir)?;
    }

    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.json");
    let tmp_name = alloc::format!(".{file_name}.{}.{seq}.tmp", std::process::id());
    let tmp_path = match parent {
        Some(dir) => dir.join(tmp_name),
        None => PathBuf::from(tmp_name),
    };

    if let Err(err) = write_and_rename(&tmp_path, path, serialized.as_bytes()) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(UserConfigError::Io(err));
    }

    Ok(current)
}

fn write_and_rename(tmp_path: &Path, target_path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(tmp_path, bytes)?;
    std::fs::rename(tmp_path, target_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn read_missing_returns_defaults_and_put_shallow_merges_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let cfg_path = dir.path().join("nested").join("config.json");
        let store = UserConfigStore::new(Some(cfg_path.clone()));

        assert_eq!(store.read().await, UserConfig::default());

        let first = store
            .update_client(json!({
                "client": {
                    "appearance": { "theme": "light", "fontSize": 15 },
                    "customFlag": true
                }
            }))
            .await
            .unwrap();
        assert_eq!(first.version, 1);
        assert_eq!(first.client["customFlag"], json!(true));

        let second = store
            .update_client(json!({
                "client": {
                    "appearance": { "theme": "dark", "fontSize": 16 }
                }
            }))
            .await
            .unwrap();
        assert_eq!(second.client["customFlag"], json!(true));
        assert_eq!(second.client["appearance"]["theme"], json!("dark"));
        assert_eq!(store.read().await, second);
    }

    #[tokio::test]
    async fn rejects_non_object_and_oversized_patches() {
        let dir = tempfile::tempdir().unwrap();
        let store = UserConfigStore::new(Some(dir.path().join("config.json")));

        let bad = store.update_client(json!({ "client": [] })).await;
        assert!(matches!(bad, Err(UserConfigError::InvalidPatch(_))));

        let huge_str = "x".repeat(MAX_USER_CONFIG_BYTES + 128);
        let too_large = store
            .update_client(json!({ "client": { "blob": huge_str } }))
            .await;
        assert!(matches!(too_large, Err(UserConfigError::TooLarge)));
    }
}
