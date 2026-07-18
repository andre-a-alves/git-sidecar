# git-sidecar

[![Crates.io](https://img.shields.io/crates/v/git-sidecar.svg)](https://crates.io/crates/git-sidecar)
[![CI](https://github.com/andre-a-alves/git-sidecar/actions/workflows/ci.yml/badge.svg)](https://github.com/andre-a-alves/git-sidecar/actions/workflows/ci.yml)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](https://github.com/andre-a-alves/git-sidecar#license)

Run git commands against sidecar repositories that live inside your working directory.

A sidecar repo is a separate git repository checked out inside another project — useful for keeping vendored code, generated outputs, or loosely related projects alongside your main repo without making them submodules.

## Installation

```
cargo install git-sidecar
```

This installs the `git-sidecar` binary. Git automatically treats any `git-<name>` binary on your `PATH` as a subcommand, so it becomes available as `git sidecar`.

## Configuration

Configuration is stored outside the project, under your OS config directory:

- Linux: `$XDG_CONFIG_HOME/git-sidecar`, or `~/.config/git-sidecar` when `XDG_CONFIG_HOME` is not set
- macOS: `~/Library/Application Support/git-sidecar`
- Windows: `%APPDATA%\git-sidecar`

`git-sidecar` identifies the current project from the nearest Git repository's `remote.origin.url`.
The remote URL is normalized into a repo-shaped config path. For example, a parent repo with this origin:

```
git@github.com:andre-a-alves/git-sidecar.git
```

uses this config file on Linux when `XDG_CONFIG_HOME` is not set:

```
~/.config/git-sidecar/github.com/andre-a-alves/git-sidecar/config.toml
```

The config file must use version `1`:

```toml
version = 1

[sidecars.foobar]
repo = "git@github.com:example/foobar.git"
mapping = ".vendor/foobar/"
```

- **`version`** — the config file version; currently only `1` is supported
- **`sidecars.foobar`** — the nickname you use on the command line
- **`repo`** — the remote URL for the sidecar repository
- **`mapping`** — path to the directory containing the sidecar git repository, relative to the parent repo root

You can define as many `[sidecars.<nickname>]` entries as you need. You don't have to write this file by hand — `git sidecar clone` creates and extends it for you.

## Usage

```
git sidecar <sidecar-name> <git-command> [args...]
```

`git-sidecar` finds the nearest Git repository, loads its OS config file, then runs the given git command inside the sidecar's `mapping` directory. You can run it from anywhere inside your project.

```
# List branches of the sidecar repo
git sidecar foobar branch

# Pull latest changes
git sidecar foobar pull

# View recent commits
git sidecar foobar log --oneline

# Check status
git sidecar foobar status
```

Any git command and its arguments are passed through as-is.

### Listing sidecars

```
git sidecar list [--global]
```

`git sidecar list` shows the sidecars configured for the current repo — one per line with the nickname, remote URL, and mapping. With `--global`, it instead walks the whole config directory and lists every configured sidecar, grouped under a `<host>/<owner>/<repo>:` header identifying the parent repo.

### Syncing sidecars

```
git sidecar sync [<sidecar-name>]
```

`git sidecar sync` clones every configured sidecar that is not already present into its `mapping` directory (a missing or empty directory counts as not present). Pass a sidecar name to sync just that one.

Sidecars that are already cloned are left untouched, but their `remote.origin.url` is checked against the configured `repo` — a mismatch prints a warning. If a mapping directory exists and is non-empty but isn't a git repository, it is skipped with a warning. Any warning or failed clone makes the command exit non-zero.

`sync` also makes sure every present sidecar is listed in the parent repo's exclude file (see below), so sidecar directories never show up in the parent's `git status`.

### Cloning a new sidecar

```
git sidecar clone <repo-url> [<directory>] [--name <nickname>]
```

`git sidecar clone` clones a repo into the parent repository and registers it as a sidecar — creating the config file if it doesn't exist yet. The nickname and directory both default to the repository name from the URL:

```
# Clones into ./foobar, registers as [sidecars.foobar]
git sidecar clone git@github.com:example/foobar.git

# Clones into .vendor/fb, registers as [sidecars.fb]
git sidecar clone git@github.com:example/foobar.git .vendor/fb --name fb
```

The directory is resolved relative to where you run the command (like `git clone`), and the stored `mapping` is computed relative to the parent repo root. If the nickname or mapping is already taken, or the target directory is non-empty, the command refuses and changes nothing. Existing config file content is preserved — the new entry is appended.

`clone` also adds the new directory to the parent repo's exclude file.

### Removing a sidecar

```
git sidecar remove <sidecar-name> [--delete]
git sidecar rm <sidecar-name> [--delete]      # alias
```

`git sidecar remove` deletes the sidecar's entry from the config file and its line from the exclude file's managed block. The cloned directory is left on disk by default — pass `--delete` to remove it as well (careful: this discards any unpushed work in the sidecar).

Because `list`, `sync`, `clone`, `remove`, `rm`, and `help` are subcommands, they are reserved and cannot be used as sidecar nicknames.

### The exclude file

`clone` and `sync` keep sidecar directories out of the parent repo's `git status` by adding them to `.git/info/exclude` (the local, uncommitted counterpart to `.gitignore`). Entries live in a managed block, and anything outside it is never touched:

```
# >>> git-sidecar (managed) >>>
/foobar/
/.vendor/dep2/
# <<< git-sidecar (managed) <<<
```

`clone` and `sync` only add entries; `git sidecar remove` deletes a sidecar's entry. If you edit the config by hand instead, clean up the block yourself.

## License

This project is licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.
