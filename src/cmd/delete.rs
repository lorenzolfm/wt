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
    let mut found: Vec<&Worktree> = Vec::new();
    for target in targets {
        let wt = find(&worktrees, target)?;
        // A worktree can have two names: its directory and its path.
        if !found.iter().any(|w| w.path == wt.path) {
            found.push(wt);
        }
    }

    let mut kept = Vec::new();
    for wt in found {
        if remove(repo, wt, force).is_err() {
            kept.push(wt.name());
        }
    }
    if !kept.is_empty() {
        bail!(
            "wt cannot remove {}. use --force to remove it",
            kept.join(", ")
        );
    }
    Ok(())
}

fn find<'a>(worktrees: &'a [Worktree], target: &str) -> Result<&'a Worktree> {
    worktrees
        .iter()
        .find(|w| w.name() == target || w.path.to_string_lossy() == target)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "there is no worktree with the name {target}\n  the names are: {}",
                worktrees
                    .iter()
                    .map(|w| w.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })
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
