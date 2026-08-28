use crate::config::Config;
use crate::hook;
use crate::repo::Repo;
use anyhow::Result;
use std::fs;

/// Make the store, the config entry and the hook. This command moves no files.
pub fn run(repo: &Repo) -> Result<()> {
    let mut changed = false;

    let shared = repo.shared();
    if !shared.exists() {
        fs::create_dir_all(&shared)?;
        eprintln!("  created    shared/");
        changed = true;
    }

    let key = repo.key()?;
    let mut config = Config::load()?;
    if config.repo(&key).is_none() {
        config.repo_mut(&key);
        config.save()?;
        eprintln!("  added      {key}  to {}", Config::path()?.display());
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
