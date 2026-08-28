use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The manifest. It is at `worktree-shared.toml` in the common directory.
///
/// The format is a table of named lists. A later version can add a
/// `copy = [...]` mode and keep each manifest that is already present.
/// Version 1 makes links only.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    #[serde(default)]
    pub link: Vec<String>,
}

impl Manifest {
    pub fn load(path: &Path) -> Result<Manifest> {
        if !path.exists() {
            return Ok(Manifest::default());
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("wt cannot read {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("wt cannot read the format of {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let body = toml::to_string_pretty(self)?;
        std::fs::write(path, body).with_context(|| format!("wt cannot write {}", path.display()))
    }

    pub fn contains(&self, entry: &str) -> bool {
        self.link.iter().any(|e| e == entry)
    }

    pub fn insert(&mut self, entry: &str) {
        if !self.contains(entry) {
            self.link.push(entry.to_string());
            self.link.sort();
        }
    }
}
