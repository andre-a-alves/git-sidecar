# git sidecar clone

```
git sidecar clone <repo-url> [<directory>] [--name <nickname>] [--standalone]
```

`git sidecar clone` clones a repo into the parent repository and registers it as a sidecar — creating the [config file](../configuration.md) if it doesn't exist yet. The nickname and directory both default to the repository name from the URL:

```sh
# Clones into ./foobar, registers as [sidecars.foobar]
git sidecar clone git@github.com:example/foobar.git

# Clones into .vendor/fb, registers as [sidecars.fb]
git sidecar clone git@github.com:example/foobar.git .vendor/fb --name fb
```

The directory is resolved relative to where you run the command (like `git clone`), and the stored `mapping` is computed relative to the parent repo root. If the nickname or mapping is already taken, or the target directory is non-empty, the command refuses and changes nothing. Existing config file content is preserved — the new entry is appended.

New sidecars are cloned in the [unified layout](../layout.md) by default. Pass `--standalone` to clone the traditional way (`.git` inside the mapping directory) and record `standalone = true` in the config.

`clone` also adds the new directory to the parent repo's [exclude file](../exclude-file.md).
