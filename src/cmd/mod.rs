pub mod add;
#[expect(clippy::print_stdout, reason = "the path for the shell")]
pub mod cd;
#[expect(clippy::print_stdout, reason = "the path for the shell")]
pub mod clone;
#[expect(clippy::print_stdout, reason = "the names for the completions")]
pub mod completions;
#[expect(clippy::print_stdout, reason = "the path for the shell")]
pub mod delete;
pub mod init;
#[expect(clippy::print_stdout, reason = "the list of the worktrees")]
pub mod ls;
pub mod prune;
pub mod share;
#[expect(clippy::print_stdout, reason = "the code of the shell integration")]
pub mod shell_init;
pub mod sync;
