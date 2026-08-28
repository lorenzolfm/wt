use crate::git;
use crate::manifest::Manifest;
use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

impl Worktree {
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }
}

/// A repository that has worktrees. The common directory identifies it.
///
/// The tool accepts two layouts. Git resolves both with `--git-common-dir`.
/// In the first layout the bare repository is the parent (`repo.git`).
/// In the second layout the bare repository is `repo/.bare`.
#[derive(Debug, Clone)]
pub struct Repo {
    pub common: PathBuf,
}

impl Repo {
    pub fn discover(from: Option<&Path>) -> Result<Repo> {
        let cwd = std::env::current_dir()?;
        let start = from.unwrap_or(&cwd);
        let common = git::out(start, &["rev-parse", "--path-format=absolute", "--git-common-dir"])
            .with_context(|| format!("there is no git repository at {}", start.display()))?;
        Ok(Repo {
            common: PathBuf::from(common),
        })
    }

    pub fn shared(&self) -> PathBuf {
        self.common.join("shared")
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.common.join("worktree-shared.toml")
    }

    pub fn hook_path(&self) -> PathBuf {
        self.common.join("hooks").join("post-checkout")
    }

    pub fn is_managed(&self) -> bool {
        self.manifest_path().exists()
    }

    pub fn manifest(&self) -> Result<Manifest> {
        Manifest::load(&self.manifest_path())
    }

    /// The directory for new worktrees. It is beside `.bare`, or it is the
    /// bare repository itself in the first layout.
    pub fn container(&self) -> PathBuf {
        if self.common.file_name().map(|n| n == ".bare").unwrap_or(false) {
            self.common
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.common.clone())
        } else {
            self.common.clone()
        }
    }

    /// Each worktree. The list does not contain the bare repository.
    pub fn worktrees(&self) -> Result<Vec<Worktree>> {
        let text = git::out(&self.common, &["worktree", "list", "--porcelain"])?;
        let mut found = Vec::new();
        let mut path: Option<PathBuf> = None;
        let mut branch: Option<String> = None;
        let mut bare = false;

        let mut flush = |path: &mut Option<PathBuf>, branch: &mut Option<String>, bare: &mut bool| {
            if let Some(p) = path.take()
                && !*bare
            {
                found.push(Worktree {
                    path: p,
                    branch: branch.take(),
                });
            }
            *branch = None;
            *bare = false;
        };

        for line in text.lines() {
            if line.is_empty() {
                flush(&mut path, &mut branch, &mut bare);
            } else if let Some(p) = line.strip_prefix("worktree ") {
                flush(&mut path, &mut branch, &mut bare);
                path = Some(PathBuf::from(p));
            } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            } else if line == "bare" {
                bare = true;
            }
        }
        flush(&mut path, &mut branch, &mut bare);
        Ok(found)
    }

    /// The base branch for a new branch. It comes from the HEAD of the remote.
    pub fn default_base(&self) -> Result<String> {
        if let Ok(r) = git::out(&self.common, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
            && !r.is_empty()
        {
            return Ok(r);
        }
        for candidate in ["origin/main", "origin/master"] {
            if git::ok(&self.common, &["rev-parse", "--verify", "--quiet", candidate]) {
                return Ok(candidate.to_string());
            }
        }
        bail!("cannot determine a base branch: origin/HEAD is unset and neither origin/main nor origin/master exists")
    }

    pub fn require_managed(&self) -> Result<()> {
        if !self.is_managed() {
            bail!(
                "wt does not control {}. the file worktree-shared.toml is not present\n  run `wt init` first",
                self.common.display()
            );
        }
        Ok(())
    }
}
