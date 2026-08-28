use clap::CommandFactory;

pub mod add;
pub mod cd;
pub mod clone;
pub mod completions;
pub mod delete;
pub mod init;
pub mod ls;
pub mod share;
pub mod shell_init;
pub mod sync;

/// True when `name` is a command of wt.
///
/// A command always wins over a worktree with the same name, because clap
/// reads the first word as a command before it reads it as a worktree.
pub fn is_command(name: &str) -> bool {
    name == "help"
        || crate::Cli::command()
            .get_subcommands()
            .any(|c| c.get_name() == name || c.get_all_aliases().any(|a| a == name))
}
