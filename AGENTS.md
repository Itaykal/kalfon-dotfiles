# kalfon-dotfiles — agent guide

Personal zsh dotfiles + Rust CLIs for macOS. `CLAUDE.md` holds the full repo
conventions (every shell layout rule, p10k/k9s/macos notes) — read it for detail.
This file gives the layout at a glance and documents the release/versioning flow.

## Layout (three buckets by concern)

The root holds only entry points (`.entry`, `install.sh`, `Brewfile`) and docs.
Everything else lives in exactly one bucket:

| Bucket | What | How it's used |
|--------|------|---------------|
| `zsh/` | Shell config: `init/ vars/ aliases/ functions/ p10k/` | Sourced by `.entry`, in that order |
| `tools/` | Rust/Cargo workspace (`aws-switch`, `feature`, …) | Compiled; `tools/bin` is put on `PATH` |
| `system/` | External app/OS artifacts: `k9s/`, `macos/` | Symlinked into system locations by `install.sh` — **not** sourced, **not** compiled |

Where new things go:
- New alias → `zsh/aliases/<tool>.zsh`; new function → `zsh/functions/<name>.zsh` (never mix the two in one file).
- Third-party `source` line (fzf, zoxide, direnv) → `zsh/init/`.
- New Rust tool → `tools/crates/<tool>/` with a `[[bin]]` (CI + `install.sh` auto-discover it).
- New external artifact → `system/<app>/`, then add a symlink step to `install.sh`.

Gotchas: **don't move `tools/`** (CI, `Makefile`, and `~/.config/*` symlink sources
assume it's at the root). `.entry` computes its base as `${0:A:h}/zsh`. `zsh/vars/path.zsh`
climbs three levels (`zsh/vars/ → root`) to put `tools/bin` on `PATH` — move it and you
must fix the `:h` count or the CLIs silently leave `PATH`.

## Releasing the Rust tools (`tools/`)

The Rust CLIs under `tools/` (`feature`, `aws-switch`, …) ship from **one tag → one
release**, but each tool is a **standalone package** (its own tarball asset). One
version covers all tools.

To cut a release, push a `v<version>` tag:

```sh
git tag v0.2.0
git push origin v0.2.0
```

What happens: `.github/workflows/release.yml` triggers on `v*` tags, builds the whole
workspace (`cargo build --release` on a `macos-14`/`aarch64-apple-darwin` runner), then
packages **every** `[[bin]]` crate under `tools/crates/` into its own
`<tool>-aarch64-apple-darwin.tar.gz` and attaches all of them to a single GitHub Release
for that tag.

**New tools need no workflow edits** — add the binary crate under `tools/crates/<tool>/`;
the packaging loop discovers any crate with a `[[bin]]` automatically (library crates
like `common` are skipped).

`install.sh` auto-discovers every `[[bin]]` crate under `tools/crates/` and, on Apple
Silicon macOS with `gh` available, downloads each tool's tarball from the latest release
into `tools/bin/` via `gh release download`. The repo is **private**, so the download is
authenticated through `gh`; if `gh` is missing or not logged in (e.g. a fresh machine
before `gh auth login`), it falls back to building everything from source via
`make -C tools build`. (`gh` is in the Brewfile.)

Caveats:
- Releases are built only for `aarch64-apple-darwin`; other platforms always build locally.
- Pushing a commit/tag that adds or changes `.github/workflows/*` requires a GitHub token
  (or SSH key) with the `workflow` scope.
