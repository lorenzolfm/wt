//! Each function runs git with `args`. Git reads an argument with '-' first
//! as an option, also after a positional argument. A name from the caller or
//! from GitHub can start with '-', so it goes after `--`.

use anyhow::{Context, Result, bail};
use std::os::fd::{AsFd, OwnedFd};
use std::path::Path;
use std::process::{Command, Stdio};

/// Run git in `dir` and return its output. An error contains the stderr of git.
pub fn out(dir: &Path, args: &[&str]) -> Result<String> {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .with_context(|| format!("wt cannot run `git {}`", args.join(" ")))?;
    if !o.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&o.stdout).trim_end().to_string())
}

/// Run git and return only the result. Discard the output.
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

/// Run git and show its output to the user.
pub fn passthrough(dir: &Path, args: &[&str]) -> Result<()> {
    // git's own stdout goes to stderr: this tool reserves stdout for the
    // worktree path that the shell integration reads.
    let err: OwnedFd = std::io::stderr()
        .as_fd()
        .try_clone_to_owned()
        .context("wt cannot make a copy of the error output")?;
    let st = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .stdout(Stdio::from(err))
        .status()
        .with_context(|| format!("wt cannot run `git {}`", args.join(" ")))?;
    if !st.success() {
        bail!("git {} failed", args.join(" "));
    }
    Ok(())
}

/// Run git and discard all output. Use this function when a failure is a
/// usual result, for example the fetch operation in `wt add`.
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

/// Whether `name` is a valid branch name, by the rule of `git
/// check-ref-format --branch`. That command also reads `@{-1}` as the
/// previous branch, so the function checks the ref, the '-' and `HEAD`
/// itself.
pub fn is_branch_name(dir: &Path, name: &str) -> bool {
    !name.starts_with('-')
        && name != "HEAD"
        && ok(dir, &["check-ref-format", &format!("refs/heads/{name}")])
}
