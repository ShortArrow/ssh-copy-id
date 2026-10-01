//! Core of ssh-copy-id: pure functions shared by the CLI and its backends.

pub mod app;
pub mod cli_args;
pub mod installed_check;
pub mod key_input;
pub mod remote_script;
pub mod result_line;
