# git-shadow

Run git commands against shadow repositories that live inside your working directory.

A shadow repo is a separate git repository checked out inside another project — useful for keeping vendored code, generated outputs, or loosely related projects alongside your main repo without making them submodules.

## Installation

```
cargo install git-shadow
```

This installs the `git-shad` binary. Git automatically treats any `git-<name>` binary on your `PATH` as a subcommand, so it becomes available as `git shad`.

## Configuration

Place a `.gitshadow.toml` file at the root of your project (next to your main `.git` directory):

```toml
[[shadows]]
name = "foobar"
repo = "git@github.com:example/foobar.git"
mapping = ".vendor/foobar/"
```

- **`name`** — the alias you use on the command line
- **`repo`** — the remote URL (reserved for future use; not used yet)
- **`mapping`** — path to the directory containing the shadow git repository, relative to `.gitshadow.toml`

You can define as many `[[shadows]]` entries as you need.

## Usage

```
git shad <shadow-name> <git-command> [args...]
```

`git-shad` walks up from your current directory until it finds `.gitshadow.toml`, then runs the given git command inside the shadow's `mapping` directory. You can run it from anywhere inside your project.

```
# List branches of the shadow repo
git shad foobar branch

# Pull latest changes
git shad foobar pull

# View recent commits
git shad foobar log --oneline

# Check status
git shad foobar status
```

Any git command and its arguments are passed through as-is.

## License

This project is licensed under either the [MIT License](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option.
