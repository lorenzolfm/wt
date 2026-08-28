use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// A link to the store is already present.
    AlreadyLinked,
    /// The path was empty. The tool made a link.
    Linked,
    /// A file with the same bytes was present. The tool made a link.
    Replaced,
    /// A different file was present. The tool replaced it because of `--force`.
    Forced,
    /// A different file was present. The tool kept it.
    SkippedDivergent,
    /// The manifest gives a path, but the store does not contain it.
    MissingInStore,
}

impl Outcome {
    pub fn is_change(self) -> bool {
        matches!(self, Outcome::Linked | Outcome::Replaced | Outcome::Forced)
    }
}

/// Compare one manifest path in one worktree, and make a link.
///
/// The tool must not destroy a file that is different from the file in the
/// store. A file with the same bytes is safe to replace, because the bytes
/// stay at the target of the link.
pub fn seed_entry(shared: &Path, worktree: &Path, entry: &str, force: bool) -> Result<Outcome> {
    let src = shared.join(entry);
    let dst = worktree.join(entry);

    if !src.exists() {
        return Ok(Outcome::MissingInStore);
    }

    match fs::symlink_metadata(&dst) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            link(&src, &dst)?;
            Ok(Outcome::Linked)
        }
        Err(e) => Err(e).with_context(|| format!("wt cannot read the path {}", dst.display())),
        Ok(md) if md.file_type().is_symlink() => {
            if fs::read_link(&dst).ok().as_deref() == Some(src.as_path()) {
                Ok(Outcome::AlreadyLinked)
            } else {
                fs::remove_file(&dst)?;
                link(&src, &dst)?;
                Ok(Outcome::Linked)
            }
        }
        Ok(_) => {
            if same_content(&src, &dst)? {
                remove(&dst)?;
                link(&src, &dst)?;
                Ok(Outcome::Replaced)
            } else if force {
                remove(&dst)?;
                link(&src, &dst)?;
                Ok(Outcome::Forced)
            } else {
                Ok(Outcome::SkippedDivergent)
            }
        }
    }
}

fn link(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("wt cannot make the directory {}", parent.display()))?;
    }
    std::os::unix::fs::symlink(src, dst).with_context(|| {
        format!(
            "wt cannot make the link {} -> {}",
            dst.display(),
            src.display()
        )
    })
}

fn remove(path: &Path) -> Result<()> {
    let md = fs::symlink_metadata(path)?;
    if md.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .with_context(|| format!("wt cannot remove {}", path.display()))
}

/// Compare two paths. For a file, compare the bytes. For a directory,
/// compare the names and then compare each file.
pub fn same_content(a: &Path, b: &Path) -> Result<bool> {
    let (ma, mb) = (fs::symlink_metadata(a)?, fs::symlink_metadata(b)?);

    if ma.file_type().is_symlink() || mb.file_type().is_symlink() {
        return Ok(ma.file_type().is_symlink()
            && mb.file_type().is_symlink()
            && fs::read_link(a)? == fs::read_link(b)?);
    }
    if ma.is_dir() != mb.is_dir() {
        return Ok(false);
    }
    if !ma.is_dir() {
        return Ok(ma.len() == mb.len() && fs::read(a)? == fs::read(b)?);
    }

    let names = |d: &Path| -> Result<Vec<std::ffi::OsString>> {
        let mut v: Vec<_> = fs::read_dir(d)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|e| e.file_name())
            .collect();
        v.sort();
        Ok(v)
    };
    let (na, nb) = (names(a)?, names(b)?);
    if na != nb {
        return Ok(false);
    }
    for n in na {
        if !same_content(&a.join(&n), &b.join(&n))? {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    struct Fixture {
        _tmp: TempDir,
        shared: PathBuf,
        worktree: PathBuf,
    }

    fn fixture() -> Fixture {
        let tmp = TempDir::new().unwrap();
        let shared = tmp.path().join("shared");
        let worktree = tmp.path().join("worktree");
        fs::create_dir_all(&shared).unwrap();
        fs::create_dir_all(&worktree).unwrap();
        Fixture {
            _tmp: tmp,
            shared,
            worktree,
        }
    }

    fn write(root: &Path, rel: &str, body: &str) -> PathBuf {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn the_tool_makes_a_link_when_the_path_is_empty() {
        let f = fixture();
        write(&f.shared, ".env", "A");
        let out = seed_entry(&f.shared, &f.worktree, ".env", false).unwrap();
        assert_eq!(out, Outcome::Linked);
        assert_eq!(
            fs::read_link(f.worktree.join(".env")).unwrap(),
            f.shared.join(".env")
        );
    }

    #[test]
    fn the_tool_makes_a_link_in_a_directory_below_the_worktree() {
        let f = fixture();
        write(&f.shared, "terraform/vars.tfvars", "A");
        let out = seed_entry(&f.shared, &f.worktree, "terraform/vars.tfvars", false).unwrap();
        assert_eq!(out, Outcome::Linked);
        assert!(f.worktree.join("terraform/vars.tfvars").exists());
    }

    #[test]
    fn the_tool_keeps_a_link_that_is_correct() {
        let f = fixture();
        write(&f.shared, ".env", "A");
        seed_entry(&f.shared, &f.worktree, ".env", false).unwrap();
        let out = seed_entry(&f.shared, &f.worktree, ".env", false).unwrap();
        assert_eq!(out, Outcome::AlreadyLinked);
    }

    #[test]
    fn the_tool_points_a_different_link_to_the_store() {
        let f = fixture();
        write(&f.shared, ".env", "A");
        let other = write(&f.shared, "other", "B");
        std::os::unix::fs::symlink(&other, f.worktree.join(".env")).unwrap();
        let out = seed_entry(&f.shared, &f.worktree, ".env", false).unwrap();
        assert_eq!(out, Outcome::Linked);
        assert_eq!(
            fs::read_link(f.worktree.join(".env")).unwrap(),
            f.shared.join(".env")
        );
    }

    #[test]
    fn the_tool_replaces_a_file_that_has_the_same_bytes() {
        let f = fixture();
        write(&f.shared, ".env", "A");
        write(&f.worktree, ".env", "A");
        let out = seed_entry(&f.shared, &f.worktree, ".env", false).unwrap();
        assert_eq!(out, Outcome::Replaced);
        assert!(
            fs::symlink_metadata(f.worktree.join(".env"))
                .unwrap()
                .is_symlink()
        );
    }

    #[test]
    fn the_tool_keeps_a_file_that_is_different() {
        let f = fixture();
        write(&f.shared, ".env", "A");
        write(&f.worktree, ".env", "DIFFERENT");
        let out = seed_entry(&f.shared, &f.worktree, ".env", false).unwrap();
        assert_eq!(out, Outcome::SkippedDivergent);
        // The bytes must stay. This is the most important test in the file.
        assert_eq!(
            fs::read_to_string(f.worktree.join(".env")).unwrap(),
            "DIFFERENT"
        );
        assert!(
            !fs::symlink_metadata(f.worktree.join(".env"))
                .unwrap()
                .is_symlink()
        );
    }

    #[test]
    fn the_option_force_replaces_a_file_that_is_different() {
        let f = fixture();
        write(&f.shared, ".env", "A");
        write(&f.worktree, ".env", "DIFFERENT");
        let out = seed_entry(&f.shared, &f.worktree, ".env", true).unwrap();
        assert_eq!(out, Outcome::Forced);
        assert_eq!(fs::read_to_string(f.worktree.join(".env")).unwrap(), "A");
    }

    #[test]
    fn the_tool_reports_a_path_that_the_store_does_not_have() {
        let f = fixture();
        let out = seed_entry(&f.shared, &f.worktree, ".env", false).unwrap();
        assert_eq!(out, Outcome::MissingInStore);
        assert!(!f.worktree.join(".env").exists());
    }

    #[test]
    fn the_tool_replaces_a_directory_that_has_the_same_content() {
        let f = fixture();
        write(&f.shared, ".auth/a.json", "1");
        write(&f.shared, ".auth/b.json", "2");
        write(&f.worktree, ".auth/a.json", "1");
        write(&f.worktree, ".auth/b.json", "2");
        let out = seed_entry(&f.shared, &f.worktree, ".auth", false).unwrap();
        assert_eq!(out, Outcome::Replaced);
    }

    #[test]
    fn the_tool_keeps_a_directory_that_has_one_different_file() {
        let f = fixture();
        write(&f.shared, ".auth/a.json", "1");
        write(&f.worktree, ".auth/a.json", "CHANGED");
        let out = seed_entry(&f.shared, &f.worktree, ".auth", false).unwrap();
        assert_eq!(out, Outcome::SkippedDivergent);
        assert_eq!(
            fs::read_to_string(f.worktree.join(".auth/a.json")).unwrap(),
            "CHANGED"
        );
    }

    #[test]
    fn two_directories_with_different_names_are_not_the_same() {
        let f = fixture();
        write(&f.shared, ".auth/a.json", "1");
        write(&f.worktree, ".auth/b.json", "1");
        assert!(!same_content(&f.shared.join(".auth"), &f.worktree.join(".auth")).unwrap());
    }

    #[test]
    fn a_file_and_a_directory_are_not_the_same() {
        let f = fixture();
        write(&f.shared, "x/inner", "1");
        write(&f.worktree, "x", "1");
        assert!(!same_content(&f.shared.join("x"), &f.worktree.join("x")).unwrap());
    }

    #[test]
    fn two_files_with_different_lengths_are_not_the_same() {
        let f = fixture();
        write(&f.shared, "x", "AA");
        write(&f.worktree, "x", "A");
        assert!(!same_content(&f.shared.join("x"), &f.worktree.join("x")).unwrap());
    }

    #[test]
    fn the_tool_compares_the_target_of_two_links() {
        let f = fixture();
        let a = write(&f.shared, "target", "1");
        std::os::unix::fs::symlink(&a, f.shared.join("x")).unwrap();
        std::os::unix::fs::symlink(&a, f.worktree.join("x")).unwrap();
        assert!(same_content(&f.shared.join("x"), &f.worktree.join("x")).unwrap());
    }
}
