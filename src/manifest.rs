use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// `worktree-shared.toml`, living in the repo's common dir.
///
/// The format is a table of named lists so that a future `copy = [...]`
/// mode can be added without breaking existing manifests. v1 is link-only.
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
            .with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let body = toml::to_string_pretty(self)?;
        std::fs::write(path, body).with_context(|| format!("writing {}", path.display()))
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
