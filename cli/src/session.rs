//! The signed-in session, persisted to `~/.config/risebyday/session.json`.
//!
//! Holds the GoTrue access + refresh tokens. The access token is short-lived
//! (an hour by default); `Supabase::authed` refreshes it transparently when
//! it is about to expire and writes the new pair back here.

use crate::config::data_dir;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds, as reported by GoTrue.
    pub expires_at: u64,
    pub user_id: String,
    pub email: Option<String>,
}

impl Session {
    fn path() -> Result<PathBuf> {
        Ok(data_dir()?.join("session.json"))
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let session = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(session))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        fs::write(&path, serde_json::to_string_pretty(self)?)
            .with_context(|| format!("writing {}", path.display()))?;
        restrict_permissions(&path);
        Ok(())
    }

    pub fn clear() -> Result<()> {
        let path = Self::path()?;
        if path.exists() {
            fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
        }
        Ok(())
    }

    /// True when the access token expires within the next minute.
    pub fn needs_refresh(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.expires_at <= now + 60
    }
}

/// The file holds bearer tokens, so make it owner-only where the OS supports it.
#[cfg(unix)]
fn restrict_permissions(path: &PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &PathBuf) {}
