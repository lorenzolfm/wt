use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// The list of shared paths for one repository.
///
/// The format is a table of named lists. A later version can add a
/// `copy = [...]` mode and keep each file that is already present.
/// Version 1 makes links only.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    #[serde(default)]
    pub link: Vec<String>,
}

impl RepoConfig {
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

/// The global configuration file.
///
/// One file holds the paths for each repository. The remote URL is the key,
/// so the same file is correct on each machine. Put the file in your dotfiles
/// to keep the list of shared paths.
///
/// The file holds no secret. The store holds the content, and the store stays
/// in the repository.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub repos: BTreeMap<String, RepoConfig>,
}

impl Config {
    /// `$XDG_CONFIG_HOME/wt/config.toml`, or `~/.config/wt/config.toml`.
    pub fn path() -> Result<PathBuf> {
        let base = match std::env::var_os("XDG_CONFIG_HOME") {
            Some(x) if !x.is_empty() => PathBuf::from(x),
            _ => {
                let home = std::env::var_os("HOME").context("HOME is not set")?;
                PathBuf::from(home).join(".config")
            }
        };
        Ok(base.join("wt").join("config.toml"))
    }

    pub fn load() -> Result<Config> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("wt cannot read {}", path.display()))?;
        toml::from_str(&text)
            .with_context(|| format!("wt cannot read the format of {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("wt cannot make the directory {}", parent.display()))?;
        }
        let body = toml::to_string_pretty(self)?;
        std::fs::write(&path, body).with_context(|| format!("wt cannot write {}", path.display()))
    }

    pub fn repo(&self, key: &str) -> Option<&RepoConfig> {
        self.repos.get(key)
    }

    pub fn repo_mut(&mut self, key: &str) -> &mut RepoConfig {
        self.repos.entry(key.to_string()).or_default()
    }
}

/// Make a key from a remote URL.
///
/// The key must be the same for each form of the URL, because one machine can
/// use SSH and another machine can use HTTPS.
///
///   git@github.com:owner/repo.git      -> github.com/owner/repo
///   https://github.com/owner/repo.git  -> github.com/owner/repo
///   ssh://git@github.com/owner/repo    -> github.com/owner/repo
pub fn key_from_url(url: &str) -> String {
    let mut s = url.trim().trim_end_matches('/');

    for prefix in ["ssh://", "https://", "http://", "git://"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest;
            break;
        }
    }
    // Remove any `user@` part.
    let mut out = match s.split_once('@') {
        Some((_, rest)) => rest.to_string(),
        None => s.to_string(),
    };
    // The SCP form uses a colon between the host and the path.
    if let Some((host, path)) = out.split_once(':') {
        // Keep a port number as part of the host.
        if !path.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            out = format!("{host}/{}", path.trim_start_matches('/'));
        }
    }
    out.trim_end_matches(".git")
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::key_from_url;

    #[test]
    fn each_url_form_gives_the_same_key() {
        for url in [
            "git@github.com:owner/repo.git",
            "git@github.com:owner/repo",
            "https://github.com/owner/repo.git",
            "https://github.com/owner/repo",
            "ssh://git@github.com/owner/repo.git",
            "https://github.com/owner/repo/",
        ] {
            assert_eq!(key_from_url(url), "github.com/owner/repo", "for {url}");
        }
    }

    #[test]
    fn the_key_keeps_the_host_and_the_path() {
        assert_eq!(
            key_from_url("git@gitlab.example.com:group/sub/repo.git"),
            "gitlab.example.com/group/sub/repo"
        );
    }

    #[test]
    fn two_repositories_do_not_share_a_key() {
        assert_ne!(
            key_from_url("git@github.com:owner/one.git"),
            key_from_url("git@github.com:owner/two.git")
        );
    }
}
