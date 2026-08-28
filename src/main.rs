mod cmd;
mod git;
mod hook;
mod manifest;
mod repo;
mod seed;

use anyhow::Result;
use clap::{Parser, Subcommand};
use repo::Repo;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "wt",
    about = "Git worktrees with shared gitignored files",
    version
)]
struct Cli {
    /// Act on this repository instead of the one containing the cwd
    #[arg(long, global = true, value_name = "PATH")]
    repo: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Scaffold the shared store, manifest and post-checkout hook
    Init,

    /// Move gitignored paths into the store and link them everywhere
    Share {
        /// Paths, relative to the current worktree
        #[arg(required = true)]
        paths: Vec<String>,
        /// Replace divergent copies instead of skipping them
        #[arg(long)]
        force: bool,
    },

    /// Reconcile every worktree against the manifest
    Sync {
        /// Replace divergent copies instead of skipping them
        #[arg(long)]
        force: bool,
    },

    /// Create a worktree for a branch
    Add {
        branch: String,
        /// Directory name (default: branch with '/' replaced by '-')
        dir: Option<String>,
        /// Fetch before resolving, even if the branch is known
        #[arg(long, conflicts_with = "no_fetch")]
        fetch: bool,
        /// Never touch the network
        #[arg(long)]
        no_fetch: bool,
    },

    /// Remove a worktree and delete its branch if merged
    Delete {
        worktree: String,
        /// Remove even with modified or untracked files
        #[arg(long)]
        force: bool,
    },

    /// Clone into the .bare worktree layout
    Clone {
        url: String,
        dir: Option<String>,
    },

    /// Print shell integration (enables `cd` into new worktrees)
    ShellInit {
        #[arg(default_value = "fish")]
        shell: String,
    },

    #[command(hide = true, name = "__branches")]
    Branches,

    #[command(hide = true, name = "__worktrees")]
    Worktrees,
}

fn main() -> ExitCode {
    // Multi-call: git invokes the hook through a symlink named post-checkout,
    // with cwd set to the worktree.
    let argv: Vec<String> = std::env::args().collect();
    let invoked_as = argv
        .first()
        .map(|a| Path::new(a).file_name().unwrap_or_default().to_string_lossy().into_owned())
        .unwrap_or_default();
    if invoked_as == "post-checkout" {
        return ExitCode::from(hook::run_as_hook(&argv[1..]) as u8);
    }

    match dispatch() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("wt: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch() -> Result<()> {
    let cli = Cli::parse();
    let at = cli.repo.as_deref();

    match cli.command {
        Command::ShellInit { shell } => cmd::shell_init::run(&shell),
        Command::Clone { url, dir } => cmd::clone::run(&url, dir.as_deref()),
        other => {
            let repo = Repo::discover(at)?;
            match other {
                Command::Init => cmd::init::run(&repo),
                Command::Share { paths, force } => cmd::share::run(&repo, &paths, force),
                Command::Sync { force } => cmd::sync::run(&repo, force),
                Command::Add {
                    branch,
                    dir,
                    fetch,
                    no_fetch,
                } => {
                    let mode = if no_fetch {
                        cmd::add::Fetch::Never
                    } else if fetch {
                        cmd::add::Fetch::Always
                    } else {
                        cmd::add::Fetch::Lazy
                    };
                    cmd::add::run(&repo, &branch, dir.as_deref(), mode)
                }
                Command::Delete { worktree, force } => cmd::delete::run(&repo, &worktree, force),
                Command::Branches => cmd::delete::branches(&repo),
                Command::Worktrees => cmd::delete::names(&repo),
                Command::ShellInit { .. } | Command::Clone { .. } => unreachable!(),
            }
        }
    }
}
