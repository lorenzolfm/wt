use crate::git;
use crate::repo::Repo;
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// Make an empty repository in the `.bare` layout at `dir`, as `wt clone`
/// makes one from a remote:
///
///   <dir>/.bare/     an empty bare repository, and the common directory
///   <dir>/.git       the text `gitdir: ./.bare`
///   <dir>/<branch>   the first worktree, with an empty first commit
///
/// The branch is git's `init.defaultBranch`, or `main` when the config does
/// not set it. The first commit makes the branch, so a new branch can start
/// from it. The repository gets the store and the hook, as `wt init` makes
/// them. It has no origin, so `wt share` refuses until the user adds one.
///
/// The command refuses a `dir` that is present and not empty, and it then
/// changes nothing. When a later step fails, for example the commit with no
/// user.name, the command removes what it made, and nothing else.
///
/// The result is the first worktree. The progress goes to stderr. The
/// function prints nothing on stdout.
pub fn run(dir: &Path) -> Result<PathBuf> {
    let container = std::path::absolute(dir)
        .with_context(|| format!("wt cannot find the path of {}", dir.display()))?;
    // This check gives a clear message. The claim below is what keeps a
    // second writer out: the directory can change after the check.
    refuse_unless_empty(&container)?;

    let mut made = claim(&container)?;
    make(&container, &mut made).inspect_err(|_| {
        if let Err(e) = made.undo() {
            eprintln!(
                "warning: wt cannot remove what it made in {}: {e}",
                container.display()
            );
            for p in made.left() {
                eprintln!("  left       {}", p.display());
            }
        }
    })
}

/// Refuse a `container` that is present and is not an empty directory.
fn refuse_unless_empty(container: &Path) -> Result<()> {
    match fs::metadata(container) {
        Ok(m) if !m.is_dir() => bail!("{} is present and is not a directory", container.display()),
        Ok(_) => {
            let mut entries = fs::read_dir(container)
                .with_context(|| format!("wt cannot read {}", container.display()))?;
            if entries.next().is_some() {
                bail!("{} is present and is not empty", container.display());
            }
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("wt cannot read {}", container.display())),
    }
}

/// The paths that one call of [`run`] made. The call claims `.bare` with
/// `create_dir`, which fails when the path is present, so of two `wt new` on
/// one directory only one gets a `Made`. [`Made::undo`] removes the paths in
/// the list and nothing else, so it cannot remove the work of a different
/// writer.
struct Made {
    /// The container, when this call made it. It is then the only path to
    /// remove.
    container: Option<PathBuf>,
    /// The paths that this call made in the container, in the sequence of
    /// their creation.
    paths: Vec<PathBuf>,
}

impl Made {
    /// Remove the paths in the reverse sequence of their creation.
    fn undo(&mut self) -> std::io::Result<()> {
        if let Some(c) = &self.container {
            fs::remove_dir_all(c)?;
            self.container = None;
            self.paths.clear();
            return Ok(());
        }
        while let Some(p) = self.paths.last() {
            if fs::symlink_metadata(p)?.is_dir() {
                fs::remove_dir_all(p)?;
            } else {
                fs::remove_file(p)?;
            }
            self.paths.pop();
        }
        Ok(())
    }

    /// The paths that are still present after `undo` failed.
    fn left(&self) -> impl Iterator<Item = &PathBuf> {
        self.container.iter().chain(&self.paths)
    }
}

/// Make the container if it is absent, then claim `.bare` in it.
///
/// `create_dir` and not `create_dir_all` for the container: the second
/// accepts a directory that a different writer made after the check.
fn claim(container: &Path) -> Result<Made> {
    if let Some(parent) = container.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("wt cannot make {}", parent.display()))?;
    }
    let made_container = match fs::create_dir(container) {
        Ok(()) => true,
        Err(e) if e.kind() == ErrorKind::AlreadyExists => false,
        Err(e) => {
            return Err(e).with_context(|| format!("wt cannot make {}", container.display()));
        }
    };
    let bare = container.join(".bare");
    if let Err(e) = fs::create_dir(&bare) {
        // The container is empty: the claim failed, so this call wrote
        // nothing in it.
        if made_container {
            let _ = fs::remove_dir(container);
        }
        if e.kind() == ErrorKind::AlreadyExists {
            bail!("{} is present and is not empty", container.display());
        }
        return Err(e).with_context(|| format!("wt cannot make {}", bare.display()));
    }
    Ok(Made {
        container: made_container.then(|| container.to_path_buf()),
        paths: vec![bare],
    })
}

/// Make the repository in `container`, an empty directory that `made` holds
/// the claim of. Each path that the function makes goes into `made`.
fn make(container: &Path, made: &mut Made) -> Result<PathBuf> {
    let branch = default_branch(container)?;

    // `git init` also accepts a repository that is present, and it then
    // changes nothing. `.bare` is the empty directory of the claim.
    let bare = container.join(".bare");
    let bare_str = bare.to_string_lossy().into_owned();
    git::passthrough(
        container,
        &["init", "--bare", "-b", &branch, "--", &bare_str],
    )?;

    let dotgit = container.join(".git");
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&dotgit)
        .context("wt cannot make the .git file")?;
    made.paths.push(dotgit);
    f.write_all(b"gitdir: ./.bare\n")
        .context("wt cannot write the .git file")?;

    let repo = Repo::at(bare);
    super::init::install(&repo)?;

    // A '/' in the branch would nest the worktree, so the directory has the
    // name that `wt add` gives it.
    let dest = container.join(branch.replace('/', "-"));
    // The worktree directory is a claim too, so `made` holds it only when
    // this call made it. Git accepts an empty directory for a new worktree.
    fs::create_dir(&dest).with_context(|| format!("wt cannot make {}", dest.display()))?;
    made.paths.push(dest.clone());
    let dest_str = dest.to_string_lossy().into_owned();
    // The branch is unborn, and git refuses it as a reference. `--orphan`
    // makes a worktree on it, ready for the first commit.
    git::passthrough(
        &repo.common,
        &[
            "worktree", "add", "--orphan", "-b", &branch, "--", &dest_str,
        ],
    )?;
    // `git commit` and not `git commit-tree`: the first commit follows the
    // config of the user, for example commit.gpgSign.
    git::passthrough(&dest, &["commit", "--allow-empty", "-m", "Initial commit"])?;
    eprintln!("  worktree   {branch}");
    Ok(dest)
}

/// The branch of a new repository: git's `init.defaultBranch`, or `main`.
/// Git itself gives `master` when the config does not set it.
///
/// `git worktree add -b` gives the name to `git branch` with no `--`, so the
/// function refuses a name that is not a valid branch name.
fn default_branch(dir: &Path) -> Result<String> {
    let o = git::output(dir, &["config", "--get", "init.defaultBranch"])?;
    let set = String::from_utf8_lossy(&o.stdout).trim().to_string();
    let branch = if o.status.success() && !set.is_empty() {
        set
    } else {
        "main".to_string()
    };
    if !git::is_branch_name(dir, &branch) {
        bail!("init.defaultBranch is {branch}, and it is not a valid branch name");
    }
    Ok(branch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// A different `wt new` claimed the directory after the check of this
    /// call. The claim of this call fails, and it removes nothing of the
    /// other.
    #[test]
    fn a_directory_that_a_different_call_claimed_is_refused_and_stays() {
        let tmp = TempDir::new().unwrap();
        let container = tmp.path().join("project");
        let first = claim(&container).unwrap();
        fs::write(container.join(".bare").join("HEAD"), "").unwrap();

        assert!(claim(&container).is_err());
        assert!(first.paths[0].join("HEAD").is_file());
    }

    /// The directory was empty at the check, and a different writer added a
    /// file while the command ran, for example while git asked for a
    /// signature. The undo removes what the command made, and the file stays.
    #[test]
    fn undo_in_a_directory_that_was_present_keeps_a_file_of_a_different_writer() {
        let tmp = TempDir::new().unwrap();
        let container = tmp.path().join("project");
        fs::create_dir(&container).unwrap();
        let mut made = claim(&container).unwrap();
        fs::write(container.join("notes.txt"), "mine").unwrap();

        made.undo().unwrap();

        assert!(!container.join(".bare").exists());
        assert_eq!(
            fs::read_to_string(container.join("notes.txt")).unwrap(),
            "mine"
        );
    }

    /// The container is absent, so the claim makes it, and the undo removes it.
    #[test]
    fn undo_removes_the_directory_that_the_claim_made() {
        let tmp = TempDir::new().unwrap();
        let container = tmp.path().join("a").join("project");
        let mut made = claim(&container).unwrap();

        made.undo().unwrap();

        assert!(!container.exists());
        assert_eq!(made.left().count(), 0);
    }
}
