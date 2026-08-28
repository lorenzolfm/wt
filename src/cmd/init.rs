use crate::hook;
use crate::manifest::Manifest;
use crate::repo::Repo;
use anyhow::Result;
use std::fs;

/// Make the store, the manifest and the hook. This command moves no files.
pub fn run(repo: &Repo) -> Result<()> {
    let mut changed = false;

    let shared = repo.shared();
    if !shared.exists() {
        fs::create_dir_all(&shared)?;
        eprintln!("  created    shared/");
        changed = true;
    }

    let manifest_path = repo.manifest_path();
    if !manifest_path.exists() {
        Manifest::default().save(&manifest_path)?;
        eprintln!("  created    worktree-shared.toml");
        changed = true;
    }

    let (target, hook_changed) = hook::install(repo)?;
    if hook_changed {
        eprintln!("  installed  hooks/post-checkout -> {}", target.display());
        changed = true;
    }

    if changed {
        eprintln!("wt controls {}", repo.common.display());
    } else {
        eprintln!("wt already controls {}", repo.common.display());
    }
    Ok(())
}
