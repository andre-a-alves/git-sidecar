use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "git-sidecar",
    bin_name = "git sidecar",
    version,
    about = "Run git commands against sidecar repositories that live inside your working directory"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List the sidecars configured for this repo
    List {
        /// List every configured sidecar across all repos
        #[arg(long)]
        global: bool,
    },
    /// Clone any configured sidecars that are not present, and move
    /// old-layout git dirs to the unified external location
    Sync {
        /// Sync only this sidecar
        name: Option<String>,
        /// Mark this sidecar as standalone (its own `.git` inside the
        /// mapping directory, untouched by the parent) and move its git
        /// dir back in if needed; requires a sidecar name
        #[arg(long, conflicts_with = "unify")]
        standalone: bool,
        /// Move a standalone sidecar back to the unified layout and clear
        /// its standalone marking; requires a sidecar name
        #[arg(long)]
        unify: bool,
    },
    /// Clone a repo and register it as a sidecar of this repo
    Clone {
        /// Remote URL of the repository to clone
        repo: String,
        /// Directory to clone into, relative to the current directory
        /// (defaults to the repository name)
        directory: Option<String>,
        /// Nickname for the sidecar (defaults to the repository name)
        #[arg(long)]
        name: Option<String>,
        /// Keep the sidecar's `.git` inside the mapping directory
        /// (traditional layout) instead of the unified external git dir
        #[arg(long)]
        standalone: bool,
    },
    /// Remove a sidecar from the config and exclude file
    #[command(alias = "rm")]
    Remove {
        /// Nickname of the sidecar to remove
        name: String,
        /// Also delete the sidecar's directory
        #[arg(long)]
        delete: bool,
    },
    /// Any other first argument is a sidecar nickname: the remaining
    /// arguments are run as a git command inside that sidecar's directory
    #[command(external_subcommand)]
    Passthrough(Vec<String>),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, clap::Error> {
        Cli::try_parse_from(std::iter::once("git-sidecar").chain(args.iter().copied()))
            .map(|cli| cli.command)
    }

    #[test]
    fn list_parses_with_and_without_global() {
        assert!(matches!(
            parse(&["list"]),
            Ok(Command::List { global: false })
        ));
        assert!(matches!(
            parse(&["list", "--global"]),
            Ok(Command::List { global: true })
        ));
    }

    #[test]
    fn list_rejects_unknown_flags() {
        assert!(parse(&["list", "--bogus"]).is_err());
    }

    #[test]
    fn sync_takes_an_optional_name() {
        assert!(matches!(
            parse(&["sync"]),
            Ok(Command::Sync { name: None, .. })
        ));
        assert!(
            matches!(parse(&["sync", "cardlet"]), Ok(Command::Sync { name: Some(n), .. }) if n == "cardlet")
        );
        assert!(parse(&["sync", "a", "b"]).is_err());
    }

    #[test]
    fn sync_parses_standalone_and_unify_flags() {
        assert!(matches!(
            parse(&["sync", "cardlet", "--standalone"]),
            Ok(Command::Sync {
                standalone: true,
                unify: false,
                ..
            })
        ));
        assert!(matches!(
            parse(&["sync", "cardlet", "--unify"]),
            Ok(Command::Sync {
                standalone: false,
                unify: true,
                ..
            })
        ));
    }

    #[test]
    fn sync_rejects_standalone_together_with_unify() {
        assert!(parse(&["sync", "cardlet", "--standalone", "--unify"]).is_err());
    }

    #[test]
    fn clone_parses_url_directory_and_name() {
        let Ok(Command::Clone {
            repo,
            directory,
            name,
            standalone,
        }) = parse(&["clone", "url", ".vendor/fb", "--name", "fb"])
        else {
            panic!("expected clone command");
        };

        assert_eq!(repo, "url");
        assert_eq!(directory.as_deref(), Some(".vendor/fb"));
        assert_eq!(name.as_deref(), Some("fb"));
        assert!(!standalone);

        assert!(matches!(
            parse(&["clone", "url", "--name=fb"]),
            Ok(Command::Clone {
                directory: None,
                ..
            })
        ));
        assert!(matches!(
            parse(&["clone", "url", "--standalone"]),
            Ok(Command::Clone {
                standalone: true,
                ..
            })
        ));
        assert!(parse(&["clone"]).is_err());
        assert!(parse(&["clone", "url", "dir", "extra"]).is_err());
        assert!(parse(&["clone", "url", "--name"]).is_err());
    }

    #[test]
    fn remove_parses_name_delete_flag_and_rm_alias() {
        assert!(matches!(
            parse(&["remove", "cardlet"]),
            Ok(Command::Remove { delete: false, .. })
        ));
        assert!(matches!(
            parse(&["rm", "cardlet", "--delete"]),
            Ok(Command::Remove { delete: true, .. })
        ));
        assert!(parse(&["remove"]).is_err());
        assert!(parse(&["remove", "a", "b"]).is_err());
    }

    #[test]
    fn unknown_subcommand_falls_through_to_passthrough() {
        let Ok(Command::Passthrough(args)) = parse(&["cardlet", "status", "-sb"]) else {
            panic!("expected passthrough");
        };

        assert_eq!(args, vec!["cardlet", "status", "-sb"]);
    }

    #[test]
    fn no_arguments_is_an_error() {
        assert!(parse(&[]).is_err());
    }
}
