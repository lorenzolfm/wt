//! The code of wt. The `wt` binary uses it, and a different program can
//! make a worktree with it when `wt` is not on the PATH.
//!
//! [`cmd::add::run`] makes a worktree and returns its path. It prints its
//! progress on stderr and nothing on stdout.
//!
//! The post-checkout hook makes the links in a new worktree, and the hook
//! runs the `wt` binary. A worktree from the library therefore gets its links
//! only when `wt` is installed.

pub mod cmd;
mod config;
mod git;
pub mod hook;
pub mod repo;
mod seed;
#[cfg(test)]
mod testing;
