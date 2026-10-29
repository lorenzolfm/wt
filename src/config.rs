use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
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
/// to keep the list of shared paths. The case of a key does not matter, so
/// the file has one entry for each repository.
///
/// The file holds no secret. The store holds the content, and the store stays
/// in the repository.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, deserialize_with = "one_entry_for_each_repo")]
    pub repos: BTreeMap<RepoKey, RepoConfig>,
}

/// The entries of the file. Two keys in a different case are one repository,
/// and the map would keep the list of the second one only.
fn one_entry_for_each_repo<'de, D>(d: D) -> Result<BTreeMap<RepoKey, RepoConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut repos = BTreeMap::new();
    for (key, cfg) in BTreeMap::<String, RepoConfig>::deserialize(d)? {
        let key = RepoKey::from_url(&key);
        if let Some((was, _)) = repos.get_key_value(&key) {
            return Err(serde::de::Error::custom(format!(
                "{was} and {key} are the same repository. merge them into one entry"
            )));
        }
        repos.insert(key, cfg);
    }
    Ok(repos)
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

    pub fn repo(&self, key: &RepoKey) -> Option<&RepoConfig> {
        self.repos.get(key)
    }

    pub fn repo_mut(&mut self, key: &RepoKey) -> &mut RepoConfig {
        self.repos.entry(key.clone()).or_default()
    }
}

/// A repository: the host and the path of its remote URL, in a normal form.
///
/// GitHub ignores the case of the owner and of the name, and it gives them in
/// the case of the page. Two keys that differ only in case are therefore the
/// same repository, and `==` says so. The key keeps the case that it came
/// with, for the output and for the config file.
///
/// A host that does not ignore case, for example a plain SSH server, can
/// have two repositories whose names differ only in case. wt reads them as
/// one repository.
///
/// Do not derive `PartialEq`, `Ord` or `Hash`: a derive compares the case.
/// `from_url` is the only constructor, so each key has the normal form.
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct RepoKey(String);

impl RepoKey {
    /// The key must be the same for each form of the URL, because one machine
    /// can use SSH and another machine can use HTTPS.
    ///
    ///   git@github.com:owner/repo.git      -> github.com/owner/repo
    ///   https://github.com/owner/repo.git  -> github.com/owner/repo
    ///   ssh://git@github.com/owner/repo    -> github.com/owner/repo
    pub fn from_url(url: &str) -> RepoKey {
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
        RepoKey(
            out.trim_end_matches(".git")
                .trim_end_matches('/')
                .to_string(),
        )
    }

    /// The host, and the path on that host.
    pub fn host_and_path(&self) -> Option<(&str, &str)> {
        self.0.split_once('/')
    }

    fn folded(&self) -> impl Iterator<Item = u8> + '_ {
        self.0.bytes().map(|b| b.to_ascii_lowercase())
    }
}

impl PartialEq for RepoKey {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}

impl Eq for RepoKey {}

// The order must agree with `eq`, or a map can hold two equal keys.
impl Ord for RepoKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.folded().cmp(other.folded())
    }
}

impl PartialOrd for RepoKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for RepoKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(
                RepoKey::from_url(url).to_string(),
                "github.com/owner/repo",
                "for {url}"
            );
        }
    }

    #[test]
    fn the_key_keeps_the_host_and_the_path() {
        assert_eq!(
            RepoKey::from_url("git@gitlab.example.com:group/sub/repo.git").to_string(),
            "gitlab.example.com/group/sub/repo"
        );
    }

    #[test]
    fn two_repositories_do_not_share_a_key() {
        assert_ne!(
            RepoKey::from_url("git@github.com:owner/one.git"),
            RepoKey::from_url("git@github.com:owner/two.git")
        );
    }

    /// Two machines can have origin in two cases, and each one can add its
    /// key. A map would keep the list of one of them only.
    #[test]
    fn a_config_with_one_repository_in_two_cases_is_refused() {
        let text = "[repos.\"github.com/owner/Floresta\"]\nlink = [\".env\"]\n\n\
                    [repos.\"github.com/owner/floresta\"]\nlink = []\n";

        let error = toml::from_str::<Config>(text).unwrap_err().to_string();

        assert!(
            error.contains("github.com/owner/Floresta and github.com/owner/floresta are the same"),
            "{error}"
        );
    }

    /// `wt init` and `wt share` on a clone whose origin has a different case
    /// use the entry that is present, and they add no second entry.
    #[test]
    fn a_key_in_a_different_case_finds_the_entry_and_keeps_its_case() {
        let mut config: Config =
            toml::from_str("[repos.\"github.com/owner/Floresta\"]\nlink = [\".env\"]\n").unwrap();
        let key = RepoKey::from_url("git@github.com:owner/floresta.git");

        assert!(config.repo(&key).unwrap().contains(".env"));
        config.repo_mut(&key).insert("data");
        let text = toml::to_string(&config).unwrap();

        assert_eq!(config.repos.len(), 1);
        assert!(
            text.contains("[repos.\"github.com/owner/Floresta\"]"),
            "{text}"
        );
    }
}
