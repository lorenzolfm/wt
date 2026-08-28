use crate::git;
use crate::repo::Repo;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::PathBuf;

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
    git::passthrough(&cwd, &["clone", "--bare", url, &bare_str])?;

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
