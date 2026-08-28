use crate::git;
use crate::hook;
use crate::repo::Repo;
use anyhow::{bail, Result};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
    /// Fetch only when the branch is unknown locally (default).
    Lazy,
    /// Never touch the network.
    Never,
    /// Refresh before resolving.
    Always,
}

/// Create a worktree for `branch`, resolving it locally, then remotely,
/// then -- only if still unknown -- over the network.
pub fn run(repo: &Repo, branch: &str, dir: Option<&str>, fetch: Fetch) -> Result<()> {
    if let Some(w) = hook::check(repo) {
        eprintln!("warning: {w}");
    }

    let name = dir.map(str::to_string).unwrap_or_else(|| branch.replace('/', "-"));
    let dest: PathBuf = repo.container().join(&name);
    if dest.exists() {
        bail!("{} already exists", dest.display());
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
        eprintln!("  branch     {branch} (tracking {remote_ref})");
        track_args(&dest_str, branch, &remote_ref)
    } else {
        // Speculative: a branch that exists nowhere is the common case here,
        // so a failed fetch is expected and its output is noise.
        let found_remotely = fetch != Fetch::Never && {
            git::quiet(&repo.common, &["fetch", "origin", branch]);
            has_remote(repo, &remote_ref)
        };

        if found_remotely {
            eprintln!("  branch     {branch} (tracking {remote_ref}, newly fetched)");
            track_args(&dest_str, branch, &remote_ref)
        } else {
            let base = repo.default_base()?;
            eprintln!("  branch     {branch} (new, from {base})");
            // --no-track: a new branch cut from origin/master must not
            // inherit master as its upstream, or `git push` targets master.
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

    // The post-checkout hook seeds; stdout carries the path for `cd`.
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
        &["show-ref", "--verify", "--quiet", &format!("refs/heads/{branch}")],
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
