//! The code of wt. The `wt` binary uses it, and a different program can
//! make a worktree with it when `wt` is not on the PATH.
//!
//! [`repo::Repo::discover`] finds the repository of a directory.
//! [`cmd::add::run`] makes a worktree in it and returns the worktree. It
//! prints its progress on stderr and nothing on stdout.
//!
//! The post-checkout hook makes the links in a new worktree, and the hook
//! runs the `wt` binary. A worktree from the library therefore gets its links
//! only when `wt` is installed.

// The library prints nothing on stdout. The commands that print a path for
// the shell are the exceptions, and cmd/mod.rs lists them.
#![deny(clippy::print_stdout)]

pub mod cmd;
mod config;
mod git;
pub mod hook;
pub mod repo;
mod seed;
#[cfg(test)]
mod testing;
