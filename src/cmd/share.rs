use crate::git;
use crate::repo::Repo;
use crate::seed::{Outcome, seed_entry};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// Move ignored files into the store. Then make a link in each worktree.
pub fn run(repo: &Repo, paths: &[String], force: bool) -> Result<()> {
    repo.require_managed()?;
    let cwd = std::env::current_dir()?;
    let toplevel = PathBuf::from(git::out(&cwd, &["rev-parse", "--show-toplevel"])?);
    let mut manifest = repo.manifest()?;

    for raw in paths {
        match share_one(repo, &toplevel, &cwd, raw, &mut manifest, force) {
            Ok(()) => {}
            Err(e) => eprintln!("error: {raw}: {e:#}"),
        }
    }

    manifest.save(&repo.manifest_path())?;
    Ok(())
}

fn share_one(
    repo: &Repo,
    toplevel: &Path,
    cwd: &Path,
    raw: &str,
    manifest: &mut crate::manifest::Manifest,
    force: bool,
) -> Result<()> {
    let entry = relative_entry(toplevel, cwd, raw)?;
    let source = toplevel.join(&entry);
    let stored = repo.shared().join(&entry);

    if manifest.contains(&entry) {
        bail!("the manifest contains this path");
    }

    // Git supplies each tracked path. A link over a tracked path hides the
    // checkout. This test also refuses `wt share terraform` when git ignores
    // only terraform/terraform.tfvars.
    let tracked = git::out(toplevel, &["ls-files", "--", &entry])?;
    if !tracked.is_empty() {
        let first = tracked.lines().next().unwrap_or_default();
        bail!(
            "this path contains a tracked file, for example {first}\n  \
             git supplies each tracked file. give the ignored path instead"
        );
    }

    if !git::ok(toplevel, &["check-ignore", "-q", "--", &entry]) {
        bail!("git does not ignore this path. wt controls only the ignored paths");
    }

    if !source.exists() {
        bail!("this path is not in {}", toplevel.display());
    }
    if fs::symlink_metadata(&source)?.file_type().is_symlink() {
        bail!("this path is already a link");
    }
    if stored.exists() {
        bail!("the store already contains shared/{entry}");
    }

    if let Some(parent) = stored.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&source, &stored)
        .with_context(|| format!("wt cannot move {} into the store", source.display()))?;
    eprintln!(
        "  moved      {} -> shared/{}",
        display_rel(toplevel, &source),
        entry
    );

    manifest.insert(&entry);

    // Make the link in each worktree. This worktree is one of them.
    let shared = repo.shared();
    let mut replaced = Vec::new();
    for wt in repo.worktrees()? {
        match seed_entry(&shared, &wt.path, &entry, force)? {
            Outcome::Linked | Outcome::Replaced | Outcome::Forced => replaced.push(wt.name()),
            Outcome::SkippedDivergent => eprintln!(
                "  skipped    {}  (a different file is present. use --force to replace it)",
                wt.name()
            ),
            Outcome::MissingInStore | Outcome::AlreadyLinked => {}
        }
    }
    if !replaced.is_empty() {
        eprintln!("  linked     {}", replaced.join(", "));
    }
    Ok(())
}

/// Change a path from the user into a path that is relative to the worktree.
fn relative_entry(toplevel: &Path, cwd: &Path, raw: &str) -> Result<String> {
    let joined = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        cwd.join(raw)
    };
    // Remove `.` and `..` from the path. The path does not need to exist.
    let mut normal = PathBuf::new();
    for part in joined.components() {
        match part {
            std::path::Component::ParentDir => {
                normal.pop();
            }
            std::path::Component::CurDir => {}
            other => normal.push(other),
        }
    }
    let rel = normal
        .strip_prefix(toplevel)
        .with_context(|| format!("{raw} is not in the worktree at {}", toplevel.display()))?;
    if rel.as_os_str().is_empty() {
        bail!("wt cannot share the top directory of the worktree");
    }
    Ok(rel.to_string_lossy().into_owned())
}

fn display_rel(base: &Path, p: &Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}
