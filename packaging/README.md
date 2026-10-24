# packaging — what ships besides the binary (`e7.4`, **reco-3q4**)

Two kinds of thing live here, and they have opposite rules.

**Generated, and never hand-edited:**

| Path | Made by | Checked by |
|---|---|---|
| `completions/bash/fathom`, `completions/zsh/_fathom`, `completions/fish/fathom.fish` | `fathom completions <shell>`, run by `tools/gen_shell_artifacts.py` | `python3 tools/gen_shell_artifacts.py check` (CI) |
| `man/man1/fathom*.1` | `fathom completions man --out packaging/man`, same tool | same |

They are the clap grammar printed, which is the only way they cannot disagree with `--help`. Regenerate
after changing a verb, a flag, or the version:

```bash
cargo build -p fathom-cli && python3 tools/gen_shell_artifacts.py write
```

**Hand-written, and reviewed as code:**

| File | What it is |
|---|---|
| `linux-packages.sh` | the command `dist` runs in its global phase to build the `.deb`s and `.rpm`s (it installs `rpmbuild`, which the runner image lacks) |

The RPM spec is not in this directory on purpose: `tools/package_release.py` generates it from the
manifest, because a spec file with the version typed into it is a second place to forget a bump.

`dist-workspace.toml`'s `include` puts `completions/` and `man/` into every release archive beside the
binary, and `tools/package_release.py` relocates the same bytes into `/usr/share/...` inside the `.deb`
and `.rpm`. Nothing in the tree is a second copy of a flag list.
