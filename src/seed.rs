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
