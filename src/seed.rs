use anyhow::{Context, Result};
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Already a symlink pointing at the store.
    AlreadyLinked,
    /// Nothing was there; link created.
    Linked,
    /// A real copy with identical bytes was swapped for a link.
    Replaced,
    /// A divergent real copy was replaced because --force was given.
    Forced,
    /// A real copy differs from the store; left untouched.
    SkippedDivergent,
    /// The manifest names an entry the store does not have.
    MissingInStore,
}

impl Outcome {
    pub fn is_change(self) -> bool {
        matches!(self, Outcome::Linked | Outcome::Replaced | Outcome::Forced)
    }
}

/// Reconcile one manifest entry in one worktree.
///
/// The invariant: never destroy a real file whose contents differ from the
/// store. Identical copies are safe to replace by definition -- the bytes
/// survive at the link target.
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
        Err(e) => Err(e).with_context(|| format!("stat {}", dst.display())),
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
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::os::unix::fs::symlink(src, dst)
        .with_context(|| format!("linking {} -> {}", dst.display(), src.display()))
}

fn remove(path: &Path) -> Result<()> {
    let md = fs::symlink_metadata(path)?;
    if md.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
    .with_context(|| format!("removing {}", path.display()))
}

/// Byte-for-byte for files, recursive name-and-content for directories.
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
