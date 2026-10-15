//! Core of cc-moss: the session registry plus agent and tmux adapters.

pub mod agent;
pub mod command;
pub mod config;
pub mod event;
pub mod launch_args;
pub mod managed_block;
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
pub mod worktree;
