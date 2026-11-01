use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, fchown};
use std::path::{Path, PathBuf};

/// The list of shared paths for one repository.
///
/// The format is a table of named lists. A later version can add a
/// `copy = [...]` mode and keep each file that is already present.
/// Version 1 makes links only.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    #[serde(default)]
    pub link: SharedPaths,
}

/// The shared paths of one repository, each one time, in order. No path is
/// inside a different one.
///
/// With a link at `.auth`, the path `.auth/token` is the file in the store,
/// and wt changes nothing below a link. A list with both paths is therefore
/// never correct. The same path two times loses nothing, so the list keeps
/// it one time, as `.auth/` is `.auth`.
///
/// `try_from` and `insert` are the only ways to add a path, so each list has
/// this form.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(try_from = "Vec<SharedPath>", into = "Vec<SharedPath>")]
pub struct SharedPaths(BTreeSet<SharedPath>);

impl SharedPaths {
    /// The path in the list that is `entry`, holds it, or is inside it.
    pub fn overlap(&self, entry: &SharedPath) -> Option<&SharedPath> {
        self.0.iter().find(|p| p.holds(entry) || entry.holds(p))
    }

    pub fn insert(&mut self, entry: SharedPath) -> Result<(), String> {
        match self.overlap(&entry) {
            None => {
                self.0.insert(entry);
                Ok(())
            }
            Some(same) if *same == entry => Ok(()),
            Some(other) => {
                let (outer, inner) = if other.holds(&entry) {
                    (other, &entry)
                } else {
                    (&entry, other)
                };
                Err(format!(
                    "{:?} is inside {:?}. keep one of them",
                    inner.as_str(),
                    outer.as_str()
                ))
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &SharedPath> {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a SharedPaths {
    type Item = &'a SharedPath;
    type IntoIter = std::collections::btree_set::Iter<'a, SharedPath>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl TryFrom<Vec<SharedPath>> for SharedPaths {
    type Error = String;

    fn try_from(paths: Vec<SharedPath>) -> Result<Self, Self::Error> {
        let mut list = SharedPaths::default();
        for p in paths {
            list.insert(p)?;
        }
        Ok(list)
    }
}

impl From<SharedPaths> for Vec<SharedPath> {
    fn from(list: SharedPaths) -> Vec<SharedPath> {
        list.0.into_iter().collect()
    }
}

/// A shared path: names below the top of the worktree, with `/` between
/// them.
///
/// wt joins the path to the store and to each worktree, and it removes the
/// file that it finds there. An absolute path gives the same file in the
/// store and in the worktree. An empty path, `.` and `..` give the worktree
/// itself or a path outside it. The constructor refuses each of them, so a
/// person can edit the config file and wt still removes nothing outside the
/// worktree.
///
/// The names cannot show a link in a directory above the path, which also
/// gives a path outside the worktree. `seed::inspect` reads the worktree for
/// that, and wt changes nothing below a link.
///
/// A `/` at the end makes the system follow a link. A correct link to a
/// directory in the store then looks like a copy of the store, and wt would
/// remove the store. The path therefore keeps only its names: `.auth/` is
/// `.auth`.
///
/// `try_from` is the only constructor, so each path has this form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct SharedPath(String);

impl SharedPath {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when `other` is this path or a path below it. The names decide:
    /// `.auth` holds `.auth/token` and not `.auth-old`.
    pub fn holds(&self, other: &SharedPath) -> bool {
        other.0 == self.0
            || other
                .0
                .strip_prefix(&self.0)
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

impl TryFrom<String> for SharedPath {
    type Error = String;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        let names: Vec<&str> = raw.split('/').filter(|n| !n.is_empty()).collect();
        if raw.starts_with('/')
            || names.is_empty()
            || names.iter().any(|n| matches!(*n, "." | ".."))
        {
            return Err(format!(
                "{raw:?} is not a path in the worktree. give it from the top of the worktree, with no `.` or `..`"
            ));
        }
        Ok(SharedPath(names.join("/")))
    }
}

impl From<SharedPath> for String {
    fn from(p: SharedPath) -> String {
        p.0
    }
}

impl AsRef<Path> for SharedPath {
    fn as_ref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl fmt::Display for SharedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The global configuration file.
///
/// One file holds the paths for each repository. The remote URL is the key,
/// so the same file is correct on each machine. Put the file in your dotfiles
/// to keep the list of shared paths. The case of a key does not matter, so
/// the file has one entry for each repository.
///
/// A repository has an entry only when it has a shared path. An entry with
/// no path is the same as no entry, so the file does not keep it.
///
/// The file holds no secret. The store holds the content, and the store stays
/// in the repository.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(
        default,
        deserialize_with = "one_entry_for_each_repo",
        serialize_with = "each_entry_with_a_path"
    )]
    pub repos: BTreeMap<RepoKey, RepoConfig>,
}

/// The entries of the file. Two keys in a different case are one repository,
/// and the map would keep the list of the second one only. An entry with no
/// path loses nothing, so the function ignores it.
fn one_entry_for_each_repo<'de, D>(d: D) -> Result<BTreeMap<RepoKey, RepoConfig>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let mut repos = BTreeMap::new();
    for (key, cfg) in BTreeMap::<String, RepoConfig>::deserialize(d)? {
        if cfg.link.is_empty() {
            continue;
        }
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

/// `wt share` makes the entry before it shares a path, and each path can
/// fail. The file then keeps no entry for that repository.
fn each_entry_with_a_path<S>(repos: &BTreeMap<RepoKey, RepoConfig>, s: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    s.collect_map(repos.iter().filter(|(_, cfg)| !cfg.link.is_empty()))
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
        self.write_to(&Self::path()?)
    }

    /// Write the file at `path` in one step. A reader, for example the hook
    /// in a different terminal, then reads the old file or the new file, and
    /// never a part of one. The new file gets the owner, the group and the
    /// mode of the old file.
    ///
    /// The file can be a link into the dotfiles. A rename over the link
    /// would put a plain file in its place, so the function writes the
    /// target of the link.
    ///
    /// In two cases the function writes the old file itself, and a reader
    /// can read a part of it:
    /// - The file has a second name, a hard link. A rename would give the
    ///   new text to one name only.
    /// - The file is a mount point, for example in a container. The system
    ///   refuses the rename.
    fn write_to(&self, path: &Path) -> Result<()> {
        let path = link_target(path)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("wt cannot make the directory {}", parent.display()))?;
        }
        let body = toml::to_string_pretty(self)?;
        let in_place = || {
            std::fs::write(&path, &body)
                .with_context(|| format!("wt cannot write {}", path.display()))
        };
        let old = std::fs::metadata(&path).ok();
        if old.as_ref().is_some_and(|md| md.nlink() > 1) {
            return in_place();
        }
        // The process id gives each wt that writes a different name. Two
        // processes in two PID namespaces, or on two hosts of one home
        // directory, can have the same pid. The second one then finds the
        // file present, and it stops: it does not write into the file or
        // remove it.
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
        let file = std::fs::File::create_new(&tmp)
            .with_context(|| format!("wt cannot write {}", tmp.display()))?;
        let written =
            write_new(file, old.as_ref(), &body).and_then(|()| std::fs::rename(&tmp, &path));
        match written {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                if e.kind() == std::io::ErrorKind::ResourceBusy {
                    return in_place();
                }
                Err(e).with_context(|| format!("wt cannot write {}", path.display()))
            }
        }
    }

    pub fn repo(&self, key: &RepoKey) -> Option<&RepoConfig> {
        self.repos.get(key)
    }

    pub fn repo_mut(&mut self, key: &RepoKey) -> &mut RepoConfig {
        self.repos.entry(key.clone()).or_default()
    }
}

/// Write `body` into the new file `file`, with the owner, the group and the
/// mode of the old file.
fn write_new(
    mut file: std::fs::File,
    old: Option<&std::fs::Metadata>,
    body: &str,
) -> std::io::Result<()> {
    // The owner and the mode come before the body. Else each user can read
    // the body of a config with mode 600 until the mode changes.
    if let Some(old) = old {
        // Keep the owner of the old file, as a write into it does. The
        // rename puts a new file in its place, and the writer owns that
        // file: after `sudo wt share`, root would own the config. Root can
        // give the file back. A user can give it only to a group of that
        // user, and else the file stays the writer's.
        let new = file.metadata()?;
        if (old.uid(), old.gid()) != (new.uid(), new.gid()) {
            match fchown(&file, Some(old.uid()), Some(old.gid())) {
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
                other => other?,
            }
        }
        // Keep the mode of the old file too. The owner comes first, because
        // a change of the owner can clear bits of the mode.
        file.set_permissions(old.permissions())?;
    }
    file.write_all(body.as_bytes())?;
    // The body must be on the disk before the rename. Else a crash of the
    // system can leave an empty file in the place of the config.
    file.sync_all()
}

/// The file at the end of the links from `path`.
///
/// The file does not need to exist: a link into the dotfiles can point to a
/// file that the dotfiles do not have yet. `canonicalize` fails on such a
/// link.
fn link_target(path: &Path) -> Result<PathBuf> {
    let mut path = path.to_path_buf();
    // The system also follows at most 40 links.
    for _ in 0..40 {
        match std::fs::symlink_metadata(&path) {
            Ok(md) if md.file_type().is_symlink() => {
                let next = std::fs::read_link(&path)
                    .with_context(|| format!("wt cannot read the link {}", path.display()))?;
                // A relative target starts at the directory of the link.
                path = match path.parent() {
                    Some(dir) => dir.join(next),
                    None => next,
                };
            }
            _ => return Ok(path),
        }
    }
    bail!("{} is a loop of links", path.display())
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

    fn path(raw: &str) -> SharedPath {
        SharedPath::try_from(raw.to_string()).unwrap()
    }

    fn names(link: &SharedPaths) -> Vec<&str> {
        link.iter().map(SharedPath::as_str).collect()
    }

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
                    [repos.\"github.com/owner/floresta\"]\nlink = [\"data\"]\n";

        let error = toml::from_str::<Config>(text).unwrap_err().to_string();

        assert!(
            error.contains("github.com/owner/Floresta and github.com/owner/floresta are the same"),
            "{error}"
        );
    }

    /// A person can edit the file. wt removes the file at each path, so a
    /// path that is not below the top of the worktree must not load.
    #[test]
    fn a_config_with_a_path_outside_the_worktree_is_refused() {
        for raw in [
            "/home/you/project/main/.env",
            "/",
            "",
            ".",
            "..",
            "../.env",
            "./.env",
            ".auth/.",
            "a/../.env",
        ] {
            let text = format!("[repos.\"github.com/owner/repo\"]\nlink = [{raw:?}]\n");

            let error = toml::from_str::<Config>(&text).unwrap_err().to_string();

            assert!(
                error.contains(&format!("{raw:?} is not a path in the worktree")),
                "{error}"
            );
        }
    }

    /// With `/` at the end, the system follows a correct link into the
    /// store, and the store looks like a copy of itself.
    #[test]
    fn a_slash_at_the_end_is_not_part_of_the_path() {
        let text = "[repos.\"github.com/owner/repo\"]\n\
                    link = [\".auth/\", \"terraform//vars.tfvars\"]\n";

        let config: Config = toml::from_str(text).unwrap();

        let link = &config
            .repo(&RepoKey::from_url("github.com/owner/repo"))
            .unwrap()
            .link;
        assert_eq!(names(link), [".auth", "terraform/vars.tfvars"]);
    }

    /// A link at `.auth` makes `.auth/token` the file in the store, so the
    /// two paths are never correct together. The order does not matter.
    #[test]
    fn a_config_with_a_path_inside_a_different_one_is_refused() {
        for link in [r#"[".auth", ".auth/token"]"#, r#"[".auth/token", ".auth"]"#] {
            let text = format!("[repos.\"github.com/owner/repo\"]\nlink = {link}\n");

            let error = toml::from_str::<Config>(&text).unwrap_err().to_string();

            assert!(
                error.contains(r#"".auth/token" is inside ".auth". keep one of them"#),
                "{error}"
            );
        }
    }

    /// The names decide, and not the text: `.auth-old` is not inside
    /// `.auth`.
    #[test]
    fn a_path_whose_text_starts_with_a_different_path_is_not_inside_it() {
        let text = "[repos.\"github.com/owner/repo\"]\nlink = [\".auth\", \".auth-old\"]\n";

        let config: Config = toml::from_str(text).unwrap();

        let link = &config
            .repo(&RepoKey::from_url("github.com/owner/repo"))
            .unwrap()
            .link;
        assert_eq!(names(link), [".auth", ".auth-old"]);
    }

    /// The same path two times loses nothing, so the list keeps it one
    /// time.
    #[test]
    fn a_path_two_times_is_one_path() {
        let text = "[repos.\"github.com/owner/repo\"]\n\
                    link = [\".env\", \".auth/\", \".env\", \".auth\"]\n";

        let config: Config = toml::from_str(text).unwrap();

        let link = &config
            .repo(&RepoKey::from_url("github.com/owner/repo"))
            .unwrap()
            .link;
        assert_eq!(names(link), [".auth", ".env"]);
    }

    /// An older `wt init` wrote an entry with no path for each repository.
    /// Such an entry is the same as no entry, also in a different case.
    #[test]
    fn an_entry_with_no_path_is_the_same_as_no_entry() {
        let text = "[repos.\"github.com/owner/Floresta\"]\nlink = []\n\n\
                    [repos.\"github.com/owner/floresta\"]\nlink = [\".env\"]\n\n\
                    [repos.\"github.com/owner/wt\"]\nlink = []\n";

        let config: Config = toml::from_str(text).unwrap();

        assert_eq!(config.repos.len(), 1);
        assert_eq!(
            names(
                &config
                    .repo(&RepoKey::from_url("github.com/owner/floresta"))
                    .unwrap()
                    .link
            ),
            [".env"]
        );
        assert!(
            config
                .repo(&RepoKey::from_url("github.com/owner/wt"))
                .is_none()
        );
    }

    fn one_path() -> Config {
        toml::from_str("[repos.\"github.com/owner/repo\"]\nlink = [\".env\"]\n").unwrap()
    }

    /// The config can be a link into the dotfiles, also before the dotfiles
    /// have the file. The file stays a link, its target gets the new text,
    /// and no temporary file stays.
    #[test]
    fn a_config_that_is_a_link_stays_a_link() {
        for target_is_present in [true, false] {
            let tmp = tempfile::TempDir::new().unwrap();
            let dotfiles = tmp.path().join("dotfiles").join("wt.toml");
            std::fs::create_dir_all(dotfiles.parent().unwrap()).unwrap();
            if target_is_present {
                std::fs::write(&dotfiles, "").unwrap();
            }
            let path = tmp.path().join("wt").join("config.toml");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::os::unix::fs::symlink("../dotfiles/wt.toml", &path).unwrap();

            one_path().write_to(&path).unwrap();

            assert!(
                std::fs::symlink_metadata(&path)
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert!(std::fs::read_to_string(&dotfiles).unwrap().contains(".env"));
            let names: Vec<_> = std::fs::read_dir(dotfiles.parent().unwrap())
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(names, ["wt.toml"]);
        }
    }

    /// A person can keep the config private with `chmod 600`.
    #[test]
    fn a_save_keeps_the_mode_of_the_file() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();

        one_path().write_to(&path).unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// The config can be a hard link into the dotfiles. Each name of the
    /// file gets the new text.
    #[test]
    fn a_config_that_is_a_hard_link_stays_one_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let dotfiles = tmp.path().join("wt.toml");
        std::fs::write(&dotfiles, "").unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::hard_link(&dotfiles, &path).unwrap();

        one_path().write_to(&path).unwrap();

        assert!(std::fs::read_to_string(&dotfiles).unwrap().contains(".env"));
    }

    /// A wt with the same pid, in a different PID namespace or on a
    /// different host of one home directory, writes the temporary file.
    #[test]
    fn the_temporary_file_of_another_process_stays_as_it_is() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        std::fs::write(&path, "old").unwrap();
        let other = tmp
            .path()
            .join(format!(".config.toml.{}.tmp", std::process::id()));
        std::fs::write(&other, "other").unwrap();

        let error = one_path().write_to(&path).unwrap_err().to_string();

        assert!(error.contains("wt cannot write"), "{error}");
        assert_eq!(std::fs::read_to_string(&other).unwrap(), "other");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
    }

    /// Each path that `wt share` gets can fail, and the entry then has no
    /// path.
    #[test]
    fn the_file_keeps_no_entry_with_no_path() {
        let mut config: Config =
            toml::from_str("[repos.\"github.com/owner/one\"]\nlink = [\".env\"]\n").unwrap();
        config.repo_mut(&RepoKey::from_url("github.com/owner/two"));

        let text = toml::to_string(&config).unwrap();

        assert!(text.contains("github.com/owner/one"), "{text}");
        assert!(!text.contains("github.com/owner/two"), "{text}");
    }

    /// `wt init` and `wt share` on a clone whose origin has a different case
    /// use the entry that is present, and they add no second entry.
    #[test]
    fn a_key_in_a_different_case_finds_the_entry_and_keeps_its_case() {
        let mut config: Config =
            toml::from_str("[repos.\"github.com/owner/Floresta\"]\nlink = [\".env\"]\n").unwrap();
        let key = RepoKey::from_url("git@github.com:owner/floresta.git");

        assert_eq!(names(&config.repo(&key).unwrap().link), [".env"]);
        config.repo_mut(&key).link.insert(path("data")).unwrap();
        let text = toml::to_string(&config).unwrap();

        assert_eq!(config.repos.len(), 1);
        assert!(
            text.contains("[repos.\"github.com/owner/Floresta\"]"),
            "{text}"
        );
    }
}
