# git sidecar remove

```
git sidecar remove <sidecar-name> [--delete]
git sidecar rm <sidecar-name> [--delete]      # alias
```

`git sidecar remove` deletes the sidecar's entry from the [config file](../configuration.md) and its line from the [exclude file](../exclude-file.md)'s managed block.

The cloned directory is left on disk by default — for a unified sidecar, its git directory is moved back inside the mapping directory first, so what remains is an ordinary standalone git repository.

Pass `--delete` to remove the directory (and, for unified sidecars, the external git directory) as well.

> **Warning:** `--delete` discards any unpushed work in the sidecar.
