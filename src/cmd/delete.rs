use crate::git;
use crate::repo::Repo;
use anyhow::{bail, Result};

/// Thin wrapper: `git worktree remove` plus `git branch -d`.
///
/// git refuses on modified tracked files and untracked non-ignored files.
/// It does *not* count ignored files, so anything gitignored and not in the
/// store goes without warning -- an accepted tradeoff for staying thin.
pub fn run(repo: &Repo, target: &str, force: bool) -> Result<()> {
    let worktrees = repo.worktrees()?;
    let found = worktrees
        .iter()
        .find(|w| w.name() == target || w.path.to_string_lossy() == target)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no worktree named {target}\n  known: {}",
                worktrees
                    .iter()
                    .map(|w| w.name())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;

    let path = found.path.to_string_lossy().into_owned();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path);
    // git's own message already explains why; don't restate it.
    if git::passthrough(&repo.common, &args).is_err() {
        bail!("could not remove {} (pass --force to delete it anyway)", found.name());
    }
    eprintln!("  removed    {}", found.name());

    if let Some(branch) = &found.branch {
        match git::out(&repo.common, &["branch", "-d", branch]) {
            Ok(_) => eprintln!("  deleted    branch {branch}"),
            Err(_) => eprintln!("  kept       branch {branch} (not fully merged)"),
        }
    }
    Ok(())
}

/// Names of every worktree, for shell completion.
pub fn names(repo: &Repo) -> Result<()> {
    for w in repo.worktrees()? {
        println!("{}", w.name());
    }
    Ok(())
}

/// Local and remote branch names, for shell completion.
pub fn branches(repo: &Repo) -> Result<()> {
    let text = git::out(
        &repo.common,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/remotes/origin",
        ],
    )?;
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines() {
        let name = line.strip_prefix("origin/").unwrap_or(line);
        if name != "HEAD" && !name.is_empty() {
            seen.insert(name.to_string());
        }
    }
    for n in seen {
        println!("{n}");
    }
    Ok(())
}
