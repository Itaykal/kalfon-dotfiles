# kalfon-dotfiles

Personal zsh dotfiles repo. All shell customisations live here — never edit `~/.p10k.zsh`, `~/.zshrc` aliases, or similar files directly.

## Structure

The root holds only entry points (`.entry`, `install.sh`, `Brewfile`) and docs. Everything else lives in one of three buckets **by concern**: `zsh/` (sourced into the shell), `tools/` (compiled Rust CLIs), `system/` (artifacts symlinked into external locations).

```
.entry              # dynamic loader — sources all *.zsh from each zsh/ dir in order
install.sh          # installer — symlinks, brew bundle, tool install, .zshrc wiring
Brewfile            # brew bundle manifest

zsh/                # everything .entry sources into the shell (load order below)
  init/             # initialization scripts that must run first (third-party integrations: fzf, zoxide, direnv, …)
  vars/             # exported env vars (incl. path.zsh — puts tools/bin on PATH)
  aliases/          # aliases only, one file per tool (no functions)
  functions/        # shell functions, one file per function
  p10k/             # Powerlevel10k overrides (files are numbered for load order)

tools/              # Rust/Cargo workspace — compiled CLIs (aws-switch, feature, …) on PATH via tools/bin
                    #   stays at the root: moving it would churn Cargo, Makefile, CI, and config symlinks

system/             # external app/OS artifacts symlinked out by install.sh — NOT sourced, NOT compiled
  k9s/              # k9s config (views.yaml, …) — symlinked into ~/Library/Application Support/k9s/
  macos/            # macOS-only artifacts (LaunchAgents, AppleScripts)
```

## Rules

- **All changes go in this repo.** Never patch `~/.p10k.zsh` or `~/.zshrc` directly.
- The three buckets are strict: shell config goes under `zsh/`, the Rust workspace stays under `tools/`, external app/OS artifacts go under `system/`. Don't put shell `*.zsh` outside `zsh/`, and don't move `tools/` (CI, `Makefile`, and `~/.config/*` symlink sources all assume it sits at the root).
- New aliases → new file `zsh/aliases/<tool>.zsh`. New functions → new file `zsh/functions/<name>.zsh`.
- Aliases and functions must not be mixed in the same file.
- `zsh/init/` is for anything that must run before the rest of the dotfiles load — typically `source` lines for third-party shell integrations (fzf key-bindings, zoxide, direnv hooks). Never put those `source` lines in `zsh/aliases/` or `zsh/functions/`.
- `.entry` lives at the root, computes `${0:A:h}/zsh` as its base, and dynamically sources `*.zsh` from each category dir in order: `init → vars → aliases → functions → p10k`.
- `zsh/vars/path.zsh` derives the repo root from its own location (`%x` then climb three levels: `zsh/vars/ → root`) to put `tools/bin` on PATH. If you move the file deeper/shallower, fix the `:h` count.
- `zsh/p10k/` files are numbered (`1-`, `2-`, …) because load order matters: kubernetes layout must be set before the AWS block inserts relative to it.
- `.p10k` overrides are sourced after `~/.p10k.zsh`, so they win. Use array manipulation to reorder prompt elements rather than redefining the full array.
- The AWS SSO session is always named `session`. The active profile is always `default`. Do not introduce new profile names.
- `AWS_ACCOUNT_NAME`, `AWS_ACCOUNT_ID`, `AWS_ROLE_NAME`, `AWS_DEFAULT_REGION` are exported by `aws-switch` and referenced in the p10k content expansion — keep them in sync if either side changes.
- `jq` and `fzf` are assumed to be installed. Do not add fallbacks for them.
- `system/` holds external app/OS artifacts. Nothing in it is sourced by `.entry` (it's not shell) or compiled (it's not Rust); `install.sh` symlinks each artifact into its system location.
- `system/k9s/` is symlinked into `~/Library/Application Support/k9s/` (e.g. `views.yaml`). Edit only via this repo. Bad column expressions in `views.yaml` fail silently in the UI — check `~/Library/Application Support/k9s/k9s.log` after changes.
- `system/macos/` — each subdir is a self-contained macOS artifact. `install.sh` symlinks LaunchAgent plists into `~/Library/LaunchAgents/` and (re)bootstraps them via `launchctl bootstrap gui/$(id -u)`. Agents that drive UI via System Events need Accessibility permission granted to `/usr/bin/osascript` — macOS will prompt on first run.

## Releasing the Rust tools (`tools/`)

The Rust CLIs under `tools/` (`feature`, `aws-switch`, …) ship from **one tag → one release**, but each tool is a **standalone package** (its own tarball asset). One version covers all tools.

To cut a release, push a `v<version>` tag:

```sh
git tag v0.2.0
git push origin v0.2.0
```

What happens: `.github/workflows/release.yml` triggers on `v*` tags, builds the whole workspace (`cargo build --release` on a `macos-14`/`aarch64-apple-darwin` runner), then packages **every** `[[bin]]` crate under `tools/crates/` into its own `<tool>-aarch64-apple-darwin.tar.gz` and attaches all of them to a single GitHub Release for that tag.

**New tools need no workflow edits** — add the binary crate under `tools/crates/<tool>/`; the packaging loop discovers any crate with a `[[bin]]` automatically (library crates like `common` are skipped).

`install.sh` auto-discovers every `[[bin]]` crate under `tools/crates/` and, on Apple Silicon macOS with `gh` available, downloads each tool's tarball from the latest release into `tools/bin/` via `gh release download`. The repo is **private**, so the download is authenticated through `gh` — if `gh` is missing or not logged in (e.g. a fresh machine before `gh auth login`), it falls back to building everything from source via `make -C tools build`.

Caveats:
- Releases are built only for `aarch64-apple-darwin`; other platforms always build locally.
- Because the repo is private, asset downloads need `gh` authenticated for an account with access (`gh` is in the Brewfile).
- Pushing a commit/tag that adds or changes `.github/workflows/*` requires a GitHub token (or SSH key) with the `workflow` scope.
