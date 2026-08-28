use anyhow::{bail, Context, Result};
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::process::{Command, Stdio};

/// Run git in `dir`, capturing stdout. Errors carry git's stderr.
pub fn out(dir: &Path, args: &[&str]) -> Result<String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .with_context(|| format!("could not run `git {}`", args.join(" ")))?;
    if !o.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim_end().to_string())
}

/// Run git for its exit status only.
pub fn ok(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run git with stdio inherited, so the user sees its progress on stderr.
pub fn passthrough(dir: &Path, args: &[&str]) -> Result<()> {
    // git's own stdout goes to stderr: this tool reserves stdout for the
    // worktree path that the shell integration reads.
    let err: OwnedFd = std::io::stderr()
        .as_fd()
        .try_clone_to_owned()
        .context("duplicating stderr")?;
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(Stdio::from(err))
        .status()
        .with_context(|| format!("could not run `git {}`", args.join(" ")))?;
    if !st.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

/// Attempt a git command, discarding all output. For speculative work such
/// as the lazy fetch, where failure is an ordinary outcome.
pub fn quiet(dir: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
