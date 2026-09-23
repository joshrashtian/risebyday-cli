//! Where the CLI finds its Supabase project.
//!
//! Same two values the desktop app reads (`VITE_SUPABASE_URL` and
//! `VITE_SUPABASE_PUBLISHABLE_KEY`), resolved in this order:
//!
//!   1. `SUPABASE_URL` / `SUPABASE_PUBLISHABLE_KEY` already in the environment
//!   2. `VITE_*` already in the environment
//!   3. a `.env` file found by walking up from the current directory
//!   4. the repo-root `.env` next to this crate (so `cargo run` works from anywhere in dev)
//!
//! Only the publishable key is ever used — RLS scopes every request to the
//! signed-in user, exactly like the app. The service-role key never belongs here.

use anyhow::{Context, Result, anyhow};
use std::env;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub url: String,
    pub publishable_key: String,
}

impl Config {
    pub fn load() -> Result<Self> {
        load_dotenv_files();

        let url = first_env(&["SUPABASE_URL", "VITE_SUPABASE_URL"])
            .ok_or_else(|| missing("SUPABASE_URL"))?;
        let publishable_key = first_env(&[
            "SUPABASE_PUBLISHABLE_KEY",
            "SUPABASE_ANON_KEY",
            "VITE_SUPABASE_PUBLISHABLE_KEY",
        ])
        .ok_or_else(|| missing("SUPABASE_PUBLISHABLE_KEY"))?;

        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            publishable_key,
        })
    }
}

fn first_env(names: &[&str]) -> Option<String> {
    names
        .iter()
        .filter_map(|name| env::var(name).ok())
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn missing(name: &str) -> anyhow::Error {
    anyhow!(
        "{name} is not set.\n\
         Put VITE_SUPABASE_URL and VITE_SUPABASE_PUBLISHABLE_KEY in the repo's .env \
         (see supabase/README.md), or export {name} in your shell."
    )
}

/// `dotenvy::from_path` never overrides variables that are already set, so
/// the shell always wins and the first file found wins after that.
fn load_dotenv_files() {
    if let Ok(cwd) = env::current_dir() {
        for dir in cwd.ancestors() {
            if try_load(&dir.join(".env")) {
                break;
            }
        }
    }

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    try_load(&repo_root.join(".env"));
}

fn try_load(path: &PathBuf) -> bool {
    path.is_file() && dotenvy::from_path(path).is_ok()
}

/// `~/.config/risebyday/` (or the platform equivalent) — holds `session.json`.
pub fn data_dir() -> Result<PathBuf> {
    let base = dirs::config_dir().context("could not determine a config directory")?;
    Ok(base.join("risebyday"))
}
