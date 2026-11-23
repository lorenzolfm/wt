use crate::git;
use crate::repo::Repo;
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::ErrorKind;
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
/// user.name, the command removes what it made.
///
/// The result is the first worktree. The progress goes to stderr. The
/// function prints nothing on stdout.
pub fn run(dir: &Path) -> Result<PathBuf> {
    let container = std::path::absolute(dir)
        .with_context(|| format!("wt cannot find the path of {}", dir.display()))?;
    let present = match fs::metadata(&container) {
        Ok(m) if !m.is_dir() => bail!("{} is present and is not a directory", container.display()),
        Ok(_) => {
            let mut entries = fs::read_dir(&container)
                .with_context(|| format!("wt cannot read {}", container.display()))?;
            if entries.next().is_some() {
                bail!("{} is present and is not empty", container.display());
            }
            true
        }
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => {
            return Err(e).with_context(|| format!("wt cannot read {}", container.display()));
        }
    };

    if !present {
        fs::create_dir_all(&container)
            .with_context(|| format!("wt cannot make {}", container.display()))?;
    }
    make(&container).inspect_err(|_| {
        if let Err(e) = undo(&container, present) {
            eprintln!(
                "warning: wt cannot remove what it made in {}: {e}",
                container.display()
            );
        }
    })
}

/// Make the repository in `container`, an empty directory.
fn make(container: &Path) -> Result<PathBuf> {
    let branch = default_branch(container)?;

    let bare = container.join(".bare");
    let bare_str = bare.to_string_lossy().into_owned();
    git::passthrough(
        container,
        &["init", "--bare", "-b", &branch, "--", &bare_str],
    )?;

    fs::write(container.join(".git"), "gitdir: ./.bare\n")
        .context("wt cannot write the .git file")?;

    let repo = Repo::at(bare);
    super::init::install(&repo)?;

    // A '/' in the branch would nest the worktree, so the directory has the
    // name that `wt add` gives it.
    let dest = container.join(branch.replace('/', "-"));
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

/// Remove what `make` made in `container`. A directory that was present was
/// empty, so each entry in it is from `make`, and the directory stays.
fn undo(container: &Path, present: bool) -> std::io::Result<()> {
    if !present {
        return fs::remove_dir_all(container);
    }
    for entry in fs::read_dir(container)? {
        let path = entry?.path();
        if fs::symlink_metadata(&path)?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}
