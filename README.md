# git-shadow

[![Crates.io](https://img.shields.io/crates/v/git-shadow.svg)](https://crates.io/crates/git-shadow)
[![CI](https://github.com/andre-a-alves/git-shadow/actions/workflows/ci.yml/badge.svg)](https://github.com/andre-a-alves/git-shadow/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/andre-a-alves/git-shadow#license)

Run git commands against shadow repositories that live inside your working directory.

A shadow repo is a separate git repository checked out inside another project — useful for keeping vendored code, generated outputs, or loosely related projects alongside your main repo without making them submodules.

## Installation

```
cargo install git-shadow
```

This installs the `git-shadow` binary. Git automatically treats any `git-<name>` binary on your `PATH` as a subcommand, so it becomes available as `git shadow`.

## Configuration

Configuration is stored outside the project, under your OS config directory:

- Linux: `$XDG_CONFIG_HOME/git-shadow`, or `~/.config/git-shadow` when `XDG_CONFIG_HOME` is not set
- macOS: `~/Library/Application Support/git-shadow`
- Windows: `%APPDATA%\git-shadow`

`git-shadow` identifies the current project from the nearest Git repository's `remote.origin.url`.
The remote URL is normalized into a repo-shaped config path. For example, a parent repo with this origin:

```
git@github.com:andre-a-alves/git-shadow.git
```

uses this config file on Linux when `XDG_CONFIG_HOME` is not set:

```
~/.config/git-shadow/github.com/andre-a-alves/git-shadow/config.toml
```

The config file must use version `1`:

```toml
version = 1

[shadows.foobar]
repo = "git@github.com:example/foobar.git"
mapping = ".vendor/foobar/"
```

- **`version`** — the config file version; currently only `1` is supported
- **`shadows.foobar`** — the nickname you use on the command line
- **`repo`** — the remote URL for the shadow repository
- **`mapping`** — path to the directory containing the shadow git repository, relative to the parent repo root

You can define as many `[shadows.<nickname>]` entries as you need. You don't have to write this file by hand — `git shadow clone` creates and extends it for you.

## Usage

```
git shadow <shadow-name> <git-command> [args...]
```

`git-shadow` finds the nearest Git repository, loads its OS config file, then runs the given git command inside the shadow's `mapping` directory. You can run it from anywhere inside your project.

```
# List branches of the shadow repo
git shadow foobar branch

# Pull latest changes
git shadow foobar pull

# View recent commits
git shadow foobar log --oneline

# Check status
git shadow foobar status
```

Any git command and its arguments are passed through as-is.

### Listing shadows

```
git shadow list [--global]
```

`git shadow list` shows the shadows configured for the current repo — one per line with the nickname, remote URL, and mapping. With `--global`, it instead walks the whole config directory and lists every configured shadow, grouped under a `<host>/<owner>/<repo>:` header identifying the parent repo.

### Syncing shadows

```
git shadow sync [<shadow-name>]
```

`git shadow sync` clones every configured shadow that is not already present into its `mapping` directory (a missing or empty directory counts as not present). Pass a shadow name to sync just that one.

Shadows that are already cloned are left untouched, but their `remote.origin.url` is checked against the configured `repo` — a mismatch prints a warning. If a mapping directory exists and is non-empty but isn't a git repository, it is skipped with a warning. Any warning or failed clone makes the command exit non-zero.

### Cloning a new shadow

```
git shadow clone <repo-url> [<directory>] [--name <nickname>]
```

`git shadow clone` clones a repo into the parent repository and registers it as a shadow — creating the config file if it doesn't exist yet. The nickname and directory both default to the repository name from the URL:

```
# Clones into ./foobar, registers as [shadows.foobar]
git shadow clone git@github.com:example/foobar.git

# Clones into .vendor/fb, registers as [shadows.fb]
git shadow clone git@github.com:example/foobar.git .vendor/fb --name fb
```

The directory is resolved relative to where you run the command (like `git clone`), and the stored `mapping` is computed relative to the parent repo root. If the nickname or mapping is already taken, or the target directory is non-empty, the command refuses and changes nothing. Existing config file content is preserved — the new entry is appended.

Because `list`, `sync`, and `clone` are subcommands, they are reserved and cannot be used as shadow nicknames.

## License

This project is licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.
