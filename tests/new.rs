//! `wt new` makes the store and the hook. The hook points only to a program
//! with the name wt, so these tests run the `wt` binary and not the library.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use tempfile::TempDir;

/// A home with a git config of its own. The tests ignore the config of the
/// user.
struct Home {
    tmp: TempDir,
}

impl Home {
    fn new(config: &str) -> Home {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("home")).unwrap();
        fs::write(tmp.path().join("gitconfig"), config).unwrap();
        Home { tmp }
    }

    /// A user who can commit.
    fn with_identity() -> Home {
        Home::new("[user]\n\tname = wt\n\temail = wt@example.com\n")
    }

    fn path(&self) -> &Path {
        self.tmp.path()
    }

    fn env(&self, c: &mut Command) {
        c.env("HOME", self.path().join("home"))
            .env("XDG_CONFIG_HOME", self.path().join("home").join(".config"))
            .env("GIT_CONFIG_GLOBAL", self.path().join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1");
        for v in [
            "GIT_AUTHOR_NAME",
            "GIT_AUTHOR_EMAIL",
            "GIT_COMMITTER_NAME",
            "GIT_COMMITTER_EMAIL",
            "GIT_DIR",
            "GIT_WORK_TREE",
        ] {
            c.env_remove(v);
        }
    }

    /// Run `wt new <dir>` in the home.
    fn wt_new(&self, dir: &str) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_wt"));
        c.current_dir(self.path()).args(["new", dir]);
        self.env(&mut c);
        c.output().unwrap()
    }

    fn git(&self, dir: &Path, args: &[&str]) -> Output {
        let mut c = Command::new("git");
        c.arg("-C").arg(dir).args(args);
        self.env(&mut c);
        c.output().unwrap()
    }

    /// The trimmed stdout of git, which must succeed.
    fn git_out(&self, dir: &Path, args: &[&str]) -> String {
        let o = self.git(dir, args);
        assert!(
            o.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn entries(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    v.sort();
    v
}

#[test]
fn wt_new_makes_the_bare_layout_with_an_empty_first_commit() {
    let home = Home::with_identity();

    let o = home.wt_new("project");

    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let container = home.path().join("project");
    let bare = container.join(".bare");
    let main = container.join("main");
    assert_eq!(PathBuf::from(stdout(&o)), main);
    assert_eq!(
        fs::read_to_string(container.join(".git")).unwrap(),
        "gitdir: ./.bare\n"
    );
    assert_eq!(
        home.git_out(&bare, &["config", "--get", "core.bare"]),
        "true"
    );
    assert_eq!(
        home.git_out(&bare, &["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
    assert_eq!(
        home.git_out(&main, &["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
    assert_eq!(home.git_out(&main, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(
        home.git_out(&main, &["rev-parse", "HEAD^{tree}"]),
        home.git_out(&main, &["hash-object", "-t", "tree", "/dev/null"])
    );
    assert!(bare.join("shared").is_dir());
    assert!(
        fs::symlink_metadata(bare.join("hooks").join("post-checkout"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        entries(&container),
        vec![container.join(".bare"), container.join(".git"), main]
    );
}

#[test]
fn wt_new_names_the_branch_by_init_default_branch() {
    let home = Home::new(
        "[user]\n\tname = wt\n\temail = wt@example.com\n[init]\n\tdefaultBranch = trunk\n",
    );

    let o = home.wt_new("project");

    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let trunk = home.path().join("project").join("trunk");
    assert_eq!(PathBuf::from(stdout(&o)), trunk);
    assert_eq!(
        home.git_out(&trunk, &["symbolic-ref", "HEAD"]),
        "refs/heads/trunk"
    );
}

#[test]
fn wt_new_refuses_a_directory_that_is_not_empty_and_changes_nothing() {
    let home = Home::with_identity();
    let container = home.path().join("project");
    fs::create_dir(&container).unwrap();
    fs::write(container.join("notes"), "mine").unwrap();

    let o = home.wt_new("project");

    assert!(!o.status.success());
    assert_eq!(stdout(&o), "");
    assert_eq!(entries(&container), vec![container.join("notes")]);
    assert_eq!(fs::read_to_string(container.join("notes")).unwrap(), "mine");
}

#[test]
fn wt_new_uses_an_empty_directory() {
    let home = Home::with_identity();
    let container = home.path().join("project");
    fs::create_dir(&container).unwrap();

    let o = home.wt_new("project");

    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(PathBuf::from(stdout(&o)), container.join("main"));
}

/// The first commit needs user.name. Without it, the command fails late, and
/// it removes what it made: the directory it made, or the entries in the
/// empty directory that was present.
#[test]
fn a_commit_that_fails_leaves_nothing_behind() {
    let home = Home::new("[user]\n\tuseConfigOnly = true\n");
    let present = home.path().join("present");
    fs::create_dir(&present).unwrap();

    let absent = home.wt_new("absent");
    let empty = home.wt_new("present");

    assert!(!absent.status.success());
    assert!(!home.path().join("absent").exists());
    assert!(!empty.status.success());
    assert_eq!(entries(&present), Vec::<PathBuf>::new());
}

/// A repository from `wt new` has no origin, and the config finds the shared
/// paths by its URL. The hook must not print an error at each new worktree.
#[test]
fn the_hook_in_a_repository_with_no_origin_prints_nothing() {
    let home = Home::with_identity();
    assert!(home.wt_new("project").status.success());
    let bare = home.path().join("project").join(".bare");
    // The hook points to an installed wt when one is present. This test is
    // about the wt that it built.
    let hook = bare.join("hooks").join("post-checkout");
    fs::remove_file(&hook).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_wt"), &hook).unwrap();

    let o = home.git(
        &bare,
        &["worktree", "add", "-b", "feature", "../feature", "main"],
    );

    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(!err.contains("wt:"), "{err}");
}
