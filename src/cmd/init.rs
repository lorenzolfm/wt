use crate::hook;
use crate::repo::Repo;
use anyhow::Result;
use std::fs;

/// Make the store and the hook. This command moves no files.
///
/// The command writes nothing to the config. `wt share` adds the entry with
/// the first path.
pub fn run(repo: &Repo) -> Result<()> {
    // The hook and each command find the shared paths by the URL of origin.
    repo.key()?;

    let mut changed = false;

    let (target, hook_changed) = hook::install(repo)?;
    if hook_changed {
        eprintln!("  installed  hooks/post-checkout -> {}", target.display());
        changed = true;
    }

    // `is_managed` reads the store, so the store is the last write. A hook
    // that wt cannot install then leaves the repository unmanaged.
    let shared = repo.shared();
    if !shared.exists() {
        fs::create_dir_all(&shared)?;
        eprintln!("  created    shared/");
        changed = true;
    }

    if changed {
        eprintln!("wt controls {}", repo.common.display());
    } else {
        eprintln!("wt already controls {}", repo.common.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::git;
    use tempfile::TempDir;

    /// A bare repository with no remote.
    fn bare() -> (TempDir, Repo) {
        let tmp = TempDir::new().unwrap();
        git(tmp.path(), &["init", "--bare", "-b", "main", "repo.git"]);
        let repo = Repo {
            common: tmp.path().join("repo.git"),
        };
        (tmp, repo)
    }

    /// The hook and each command find the shared paths by the URL of origin.
    /// In a repository with no origin, each one of them fails.
    #[test]
    fn a_repository_with_no_origin_gets_no_store_and_no_hook() {
        let (_tmp, repo) = bare();

        assert!(run(&repo).is_err());
        assert!(!repo.is_managed());
        assert!(fs::symlink_metadata(repo.hook_path()).is_err());
    }

    /// `is_managed` reads the store. A store with no hook would give a
    /// repository whose new worktrees get no links.
    #[test]
    fn a_hook_that_wt_cannot_install_leaves_the_repository_unmanaged() {
        let (_tmp, repo) = bare();
        git(
            &repo.common,
            &[
                "remote",
                "add",
                "origin",
                "https://example.com/owner/repo.git",
            ],
        );
        let hooks = repo.hook_path().parent().unwrap().to_path_buf();
        fs::remove_dir_all(&hooks).unwrap();
        fs::write(&hooks, "").unwrap();

        assert!(run(&repo).is_err());
        assert!(!repo.is_managed());
    }
}
