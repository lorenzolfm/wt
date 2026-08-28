use crate::git;
use crate::hook;
use crate::repo::Repo;
use anyhow::{Result, bail};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
    /// Fetch only if the branch is not known. This is the default.
    Lazy,
    /// Do not use the network.
    Never,
    /// Fetch before the tool finds the branch.
    Always,
}

/// Make a worktree for `branch`.
///
/// The command finds the branch in this sequence: a local branch, then a
/// remote branch, then a fetch from the network, then a new branch.
pub fn run(repo: &Repo, branch: &str, dir: Option<&str>, fetch: Fetch) -> Result<()> {
    if let Some(w) = hook::check(repo) {
        eprintln!("warning: {w}");
    }

    let name = dir
        .map(str::to_string)
        .unwrap_or_else(|| branch.replace('/', "-"));
    let dest: PathBuf = repo.container().join(&name);
    if dest.exists() {
        bail!("{} is already present", dest.display());
    }
    let dest_str = dest.to_string_lossy().into_owned();
    let remote_ref = format!("origin/{branch}");

    if fetch == Fetch::Always {
        eprintln!("  fetching   origin {branch}");
        git::quiet(&repo.common, &["fetch", "origin", branch]);
    }

    let args: Vec<String> = if has_local(repo, branch) {
        eprintln!("  branch     {branch} (local)");
        vec!["worktree".into(), "add".into(), dest_str, branch.into()]
    } else if has_remote(repo, &remote_ref) {
        eprintln!("  branch     {branch} (tracks {remote_ref})");
        track_args(&dest_str, branch, &remote_ref)
    } else {
        // A branch that is not present is the usual condition here. A failed
        // fetch is therefore an expected result, and its output is not useful.
        let found_remotely = fetch != Fetch::Never && {
            git::quiet(&repo.common, &["fetch", "origin", branch]);
            has_remote(repo, &remote_ref)
        };

        if found_remotely {
            eprintln!("  branch     {branch} (tracks {remote_ref}, from a new fetch)");
            track_args(&dest_str, branch, &remote_ref)
        } else {
            let base = repo.default_base()?;
            eprintln!("  branch     {branch} (new branch from {base})");
            // Use --no-track. A new branch from origin/master must not keep
            // master as its upstream branch. If it does, `git push` sends the
            // commits to master.
            vec![
                "worktree".into(),
                "add".into(),
                "--no-track".into(),
                "-b".into(),
                branch.into(),
                dest_str,
                base,
            ]
        }
    };

    let argv: Vec<&str> = args.iter().map(String::as_str).collect();
    git::passthrough(&repo.common, &argv)?;

    // The hook makes the links. The output gives the path to the shell.
    println!("{}", dest.display());
    Ok(())
}

fn track_args(dest: &str, branch: &str, remote_ref: &str) -> Vec<String> {
    vec![
        "worktree".into(),
        "add".into(),
        "--track".into(),
        "-b".into(),
        branch.into(),
        dest.into(),
        remote_ref.into(),
    ]
}

fn has_local(repo: &Repo, branch: &str) -> bool {
    git::ok(
        &repo.common,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
}

fn has_remote(repo: &Repo, remote_ref: &str) -> bool {
    git::ok(
        &repo.common,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote_ref}"),
        ],
    )
}
