use std::path::Path;
use std::process::Command;

/// Run git for a fixture. The fixture ignores the config of the user.
pub fn git(dir: &Path, args: &[&str]) {
    let o = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "wt")
        .env("GIT_AUTHOR_EMAIL", "wt@example.com")
        .env("GIT_COMMITTER_NAME", "wt")
        .env("GIT_COMMITTER_EMAIL", "wt@example.com")
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
}
