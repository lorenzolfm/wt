use crate::git;
use crate::repo::Repo;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// Clone a repository into the `.bare` layout. Then make the first worktree.
///
/// The command makes this structure:
///
///   <name>/.bare/     the bare repository, and the common directory
///   <name>/.git       the text `gitdir: ./.bare`
///   <name>/<default>  the first worktree
pub fn run(url: &str, dir: Option<&str>) -> Result<()> {
    let name = dir.map(str::to_string).unwrap_or_else(|| default_name(url));
    let cwd = std::env::current_dir()?;
    let container = cwd.join(&name);
    if container.exists() {
        bail!("{} is already present", container.display());
    }

    fs::create_dir_all(&container)?;
    let bare = container.join(".bare");
    let bare_str = bare.to_string_lossy().into_owned();
    git::passthrough(&cwd, &["clone", "--bare", "--", url, &bare_str])?;

    fs::write(container.join(".git"), "gitdir: ./.bare\n")
        .context("wt cannot write the .git file")?;

    // A bare clone has no fetch refspec. The refs below refs/remotes/origin
    // therefore stay empty, and `wt add` cannot find a remote branch.
    git::out(
        &bare,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/origin/*",
        ],
    )?;
    git::passthrough(&bare, &["fetch", "origin", "--prune"])?;

    let default = git::out(&bare, &["symbolic-ref", "--short", "HEAD"])
        .unwrap_or_else(|_| "main".to_string());
    // Origin chooses its HEAD, and `git worktree add -b` gives the name to
    // `git branch` with no `--`.
    if !git::is_branch_name(&bare, &default) {
        bail!("the HEAD of origin is {default}, and it is not a valid branch name");
    }
    let _ = git::out(
        &bare,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            &format!("refs/remotes/origin/{default}"),
        ],
    );

    let repo = Repo { common: bare };
    super::init::run(&repo)?;

    let dest: PathBuf = container.join(&default);
    add_worktree(&repo, &dest, &default)?;
    eprintln!("  worktree   {default}");

    println!("{}", dest.display());
    Ok(())
}

/// Make the worktree of `branch` at `dest`. The clone of an empty repository
/// has no commit, so `branch` is unborn and git refuses it as a reference.
/// `--orphan` makes a worktree on the unborn branch, ready for the first commit.
///
/// An absent `branch` alone does not show an empty repository. The HEAD of
/// origin can name a branch that origin does not have, while other branches
/// hold the commits. An orphan worktree there would look empty, and the user
/// would not see why. The command uses `--orphan` only when the repository
/// has no ref, and git refuses an absent `branch` in each other repository.
fn add_worktree(repo: &Repo, dest: &Path, branch: &str) -> Result<()> {
    let dest = dest.to_string_lossy();
    // The symbolic ref origin/HEAD of an empty clone points to a ref that is
    // not present. Git does not list it.
    let empty = git::out(&repo.common, &["for-each-ref", "--count=1"])?.is_empty();
    let args = if empty {
        vec!["worktree", "add", "--orphan", "-b", branch, "--", &dest]
    } else {
        vec!["worktree", "add", "--", &dest, branch]
    };
    git::passthrough(&repo.common, &args)
}

fn default_name(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("repo")
        .trim_end_matches(".git")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::git;
    use tempfile::TempDir;

    /// A bare clone of an origin on main. The origin has one commit, or none.
    /// origin/HEAD points to origin/main, as `run` sets it.
    fn clone_of_origin(with_commit: bool) -> (TempDir, Repo) {
        let tmp = TempDir::new().unwrap();
        let origin = tmp.path().join("origin");
        let bare = tmp.path().join(".bare");
        git(
            tmp.path(),
            &["init", "-b", "main", origin.to_str().unwrap()],
        );
        if with_commit {
            git(&origin, &["commit", "--allow-empty", "-m", "one"]);
        }
        git(
            tmp.path(),
            &[
                "clone",
                "--bare",
                origin.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        git(
            &bare,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        (tmp, Repo { common: bare })
    }

    #[test]
    fn the_clone_of_an_empty_repository_gives_a_worktree_on_the_unborn_branch() {
        let (tmp, repo) = clone_of_origin(false);
        let dest = tmp.path().join("main");

        add_worktree(&repo, &dest, "main").unwrap();

        assert_eq!(
            crate::git::out(&dest, &["symbolic-ref", "HEAD"]).unwrap(),
            "refs/heads/main"
        );
        assert!(!crate::git::ok(
            &dest,
            &["rev-parse", "--verify", "--quiet", "HEAD"]
        ));
    }

    #[test]
    fn the_clone_of_a_repository_with_a_commit_gives_a_worktree_at_the_branch() {
        let (tmp, repo) = clone_of_origin(true);
        let dest = tmp.path().join("main");

        add_worktree(&repo, &dest, "main").unwrap();

        assert_eq!(
            crate::git::out(&dest, &["symbolic-ref", "HEAD"]).unwrap(),
            "refs/heads/main"
        );
        assert_eq!(
            crate::git::out(&dest, &["rev-parse", "HEAD"]).unwrap(),
            crate::git::out(&tmp.path().join("origin"), &["rev-parse", "HEAD"]).unwrap()
        );
    }

    /// The HEAD of origin can name a branch that origin does not have. An
    /// orphan worktree there would look like the clone of an empty repository.
    #[test]
    fn a_branch_that_a_repository_with_a_commit_does_not_have_gives_no_worktree() {
        let (tmp, repo) = clone_of_origin(true);
        let dest = tmp.path().join("master");

        assert!(add_worktree(&repo, &dest, "master").is_err());
        assert!(!dest.exists());
    }
}
