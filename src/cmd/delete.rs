use crate::git;
use crate::repo::{Repo, Worktree};
use anyhow::{Result, bail};

/// Remove each worktree with `git worktree remove`. Then delete its branch.
///
/// The command finds each name before it removes anything, so a name that
/// is wrong changes nothing. Git can refuse a worktree. The command then
/// keeps that worktree, removes the others and fails at the end.
///
/// Git refuses to remove a worktree that has a modified tracked file. Git
/// also refuses if an untracked file is present. Git does not count the
/// files that it ignores. An ignored file that is not in the store is
/// therefore lost, and the user sees no warning.
pub fn run(repo: &Repo, targets: &[String], force: bool) -> Result<()> {
    let worktrees = repo.worktrees()?;
    let found = resolve(&worktrees, targets)?;

    let mut kept = Vec::new();
    for wt in found {
        if remove(repo, wt, force).is_err() {
            kept.push(wt.name());
        }
    }
    if !kept.is_empty() {
        let names = kept.join(", ");
        // Git printed the reason, for example a lock. The option --force is
        // already on, so it cannot help.
        if force {
            bail!("wt cannot remove {names}");
        }
        let them = if kept.len() == 1 { "it" } else { "them" };
        bail!("wt cannot remove {names}. use --force to remove {them}");
    }
    Ok(())
}

/// Find the worktree of each name, in the order of the names.
///
/// The error gives each name that is wrong, so the user can correct the
/// list in one step.
fn resolve<'a>(worktrees: &'a [Worktree], targets: &[String]) -> Result<Vec<&'a Worktree>> {
    let mut found: Vec<&Worktree> = Vec::new();
    let mut wrong = Vec::new();
    for target in targets {
        let wt = worktrees
            .iter()
            .find(|w| w.name() == *target || w.path.to_string_lossy() == *target);
        match wt {
            // A worktree can have two names: its directory and its path.
            Some(wt) => {
                if !found.iter().any(|w| w.path == wt.path) {
                    found.push(wt);
                }
            }
            None => wrong.push(target.as_str()),
        }
    }

    if !wrong.is_empty() {
        let names = worktrees
            .iter()
            .map(|w| w.name())
            .collect::<Vec<_>>()
            .join(", ");
        if let [one] = wrong.as_slice() {
            bail!("there is no worktree with the name {one}\n  the names are: {names}");
        }
        bail!(
            "there are no worktrees with the names {}\n  the names are: {names}",
            wrong.join(", ")
        );
    }
    Ok(found)
}

fn remove(repo: &Repo, wt: &Worktree, force: bool) -> Result<()> {
    let path = wt.path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path);
    // Git prints the reason. Do not print it a second time.
    git::passthrough(&repo.common, &args)?;
    eprintln!("  removed    {}", wt.name());

    if let Some(branch) = &wt.branch {
        match git::out(&repo.common, &["branch", "-d", branch]) {
            Ok(_) => eprintln!("  deleted    branch {branch}"),
            Err(_) => eprintln!("  kept       branch {branch} (git did not merge it)"),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::git;
    use tempfile::TempDir;

    /// A bare repository with one worktree for each name. Each worktree is
    /// on its own branch.
    fn repo_with(names: &[&str]) -> (TempDir, Repo) {
        let tmp = TempDir::new().unwrap();
        let origin = tmp.path().join("origin");
        let bare = tmp.path().join(".bare");
        git(
            tmp.path(),
            &["init", "-b", "main", origin.to_str().unwrap()],
        );
        git(&origin, &["commit", "--allow-empty", "-m", "one"]);
        git(
            tmp.path(),
            &[
                "clone",
                "--bare",
                origin.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        for name in names {
            let path = tmp.path().join(name);
            git(
                &bare,
                &[
                    "worktree",
                    "add",
                    "-b",
                    name,
                    path.to_str().unwrap(),
                    "main",
                ],
            );
        }
        (tmp, Repo { common: bare })
    }

    fn targets(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    #[test]
    fn the_tool_gives_each_wrong_name_and_removes_nothing() {
        let (tmp, repo) = repo_with(&["a", "b"]);

        let err = run(&repo, &targets(&["a", "typo", "b", "other"]), false).unwrap_err();

        let text = format!("{err:#}");
        assert!(text.contains("typo") && text.contains("other"), "{text}");
        assert!(tmp.path().join("a").exists());
        assert!(tmp.path().join("b").exists());
    }

    /// The second `git worktree remove` of one worktree fails, so the tool
    /// must remove it one time.
    #[test]
    fn the_tool_removes_a_worktree_with_two_names_in_the_list_one_time() {
        let (tmp, repo) = repo_with(&["a"]);
        // The path as git gives it, which can differ from the path of the
        // temporary directory, for example through a link.
        let path = repo.worktrees().unwrap()[0]
            .path
            .to_string_lossy()
            .into_owned();

        run(&repo, &targets(&["a", &path]), false).unwrap();

        assert!(!tmp.path().join("a").exists());
    }

    #[test]
    fn the_tool_keeps_a_worktree_that_git_refuses_and_removes_the_others() {
        let (tmp, repo) = repo_with(&["a", "b", "c"]);
        std::fs::write(tmp.path().join("b").join("untracked"), "").unwrap();

        assert!(run(&repo, &targets(&["a", "b", "c"]), false).is_err());

        assert!(!tmp.path().join("a").exists());
        assert!(tmp.path().join("b").exists());
        assert!(!tmp.path().join("c").exists());
    }
}
