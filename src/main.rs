//! `git sidecar` — run git commands against sidecar repositories that live
//! inside your working directory.
//!
//! A sidecar repo is a separate git repository checked out inside another
//! project. Sidecars are registered per parent repo in a config file under
//! the OS config directory, keyed by the parent's `remote.origin.url`, and
//! kept out of the parent's `git status` via `.git/info/exclude`.

mod cli;
mod commands;
mod config;
mod exclude;
mod git;
mod layout;
mod paths;
mod remote;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::{Cli, Command};

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::List { global } => commands::list::run(global),
        Command::Sync {
            name,
            standalone,
            unify,
        } => commands::sync::run(name.as_deref(), standalone, unify),
        Command::Clone {
            repo,
            directory,
            name,
            standalone,
        } => commands::clone::run(&repo, directory, name, standalone),
        Command::Remove { name, delete } => commands::remove::run(&name, delete),
        Command::Passthrough(args) => commands::passthrough::run(&args),
    }
}
