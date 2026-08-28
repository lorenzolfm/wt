use crate::git;
use crate::repo::Repo;
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Print the name of each worktree.
pub fn worktrees(repo: &Repo) -> Result<()> {
    for w in repo.worktrees()? {
        println!("{}", w.name());
    }
    Ok(())
}

/// Print each local branch name and each remote branch name.
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
    let mut seen = BTreeSet::new();
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

/// Print each path that `wt share` accepts.
///
/// These are the paths that git ignores in this worktree. Git gives a whole
/// directory as one entry when it ignores all of the directory, which is the
/// depth that `wt share` needs. A path that the config file already has does
/// not appear.
pub fn shareable(repo: &Repo) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let toplevel = PathBuf::from(git::out(&cwd, &["rev-parse", "--show-toplevel"])?);
    let text = git::out(
        &toplevel,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ],
    )?;
    let already = repo.manifest().unwrap_or_default();
    for line in text.lines() {
        let path = line.trim_end_matches('/');
        if !path.is_empty() && !already.contains(path) {
            println!("{path}");
        }
    }
    Ok(())
}
