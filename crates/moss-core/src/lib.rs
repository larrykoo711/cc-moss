//! Core of cc-moss: the session registry plus agent and tmux adapters.

#[cfg(not(unix))]
compile_error!("cc-moss supports macOS and Linux only");

pub mod agent;
pub mod aliases;
pub mod archive;
pub mod ask;
pub mod command;
pub mod config;
pub mod daemon;
pub mod event;
pub mod launch_args;
pub mod managed_block;
pub mod moss;
pub mod naming;
pub mod platform;
pub mod proc;
pub mod registry;
pub mod restore;
pub mod settings_json;
pub mod slot;
pub mod store_dir;
pub mod tmux;
pub mod transcript;
pub mod trash;
pub mod update;
pub mod worktree;
