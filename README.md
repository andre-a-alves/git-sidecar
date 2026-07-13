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

You can define as many `[shadows.<nickname>]` entries as you need.

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

Because `list` is a subcommand, it is reserved and cannot be used as a shadow nickname.

## License

This project is licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.
