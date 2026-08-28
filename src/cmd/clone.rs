use crate::git;
use crate::repo::Repo;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::PathBuf;

/// Clone into the `.bare` layout and create the first worktree.
///
///   <name>/.bare/     the bare repo (common dir: store, manifest, hooks)
///   <name>/.git       "gitdir: ./.bare", so git works from the container
///   <name>/<default>  the first worktree
pub fn run(url: &str, dir: Option<&str>) -> Result<()> {
    let name = dir
        .map(str::to_string)
        .unwrap_or_else(|| default_name(url));
    let cwd = std::env::current_dir()?;
    let container = cwd.join(&name);
    if container.exists() {
        bail!("{} already exists", container.display());
    }

    fs::create_dir_all(&container)?;
    let bare = container.join(".bare");
    let bare_str = bare.to_string_lossy().into_owned();
    git::passthrough(&cwd, &["clone", "--bare", url, &bare_str])?;

    fs::write(container.join(".git"), "gitdir: ./.bare\n")
        .context("writing the .git pointer file")?;

    // A bare clone has no fetch refspec, so refs/remotes/origin/* stays empty
    // and branch resolution in `wt add` would never see remote branches.
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
    let dest_str = dest.to_string_lossy().into_owned();
    git::passthrough(&repo.common, &["worktree", "add", &dest_str, &default])?;
    eprintln!("  worktree   {default}");

    println!("{}", dest.display());
    Ok(())
}

fn default_name(url: &str) -> String {
    url.trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("repo")
        .trim_end_matches(".git")
        .to_string()
}
