# dev-tools

Personal terminal CLIs in Rust. This repo is a **single Cargo workspace** exposed
as a Nix flake. It used to also hold zsh dotfiles and macOS/system artifacts —
those moved to `Itaykal/nixos-config` (nix-darwin + Home Manager); this repo is
now just the Rust tools.

## Structure

```
Cargo.toml          workspace + shared dependency versions
rust-toolchain.toml stable channel + rustfmt/clippy components
Makefile            build / dev / test / fmt / clippy
flake.nix           Nix flake; packages.<system>.dev-tools builds all binaries
crates/
  common/           shared lib: theme, fuzzy picker, config + cache loaders,
                    term guard, spinner  (no [[bin]])
  aws-switch/        [[bin]] — AWS SSO account+role picker → ~/.aws/config
  feature/           [[bin]] — Jira issue picker/creator → git checkout -b
  wt-gc/             [[bin]] — stale git-worktree GC
```

- MSRV `rust-version = "1.96"`, edition 2021. Release profile is size-tuned
  (`opt-level = "z"`, `lto`, `strip`, `codegen-units = 1`).
- Shared dep versions live in `[workspace.dependencies]`; crates pull them in with
  `dep.workspace = true`.

## Rules

- **New tool** → add a binary crate under `crates/<tool>/` with a `[[bin]]`. The
  release CI loop, the flake, and the Makefile `TOOLS` list pick up binary crates;
  list crates with no `[[bin]]` (like `common`) are skipped by CI/flake but still
  need to be added to the workspace `members` and `Makefile TOOLS` as appropriate.
- **Shared code** goes in `crates/common/`, consumed via the workspace dependency.
- Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` before committing
  (CI builds with `--locked`, so keep `Cargo.lock` committed and current).

## Nix flake + the consumer

- `flake.nix` builds one derivation, `packages.<system>.dev-tools`, that installs
  every `[[bin]]` crate. `src = ./.` and `cargoLock.lockFile = ./Cargo.lock` point
  at the repo root (the workspace). Supported systems: `aarch64-darwin`,
  `x86_64-linux`.
- The toolchain is pinned via `rust-overlay` (`rust-bin.stable.latest.default`)
  because nixpkgs 26.05 ships rustc 1.95 < the 1.96 MSRV. Keep the nixpkgs channel
  in sync with the consumer.
- **`Itaykal/nixos-config` consumes this as the `dev-tools` flake input** and uses
  **only** `inputs.dev-tools.packages.<system>.dev-tools` (built binaries). The
  output attr name `dev-tools` and a buildable `packages.aarch64-darwin.dev-tools`
  must keep working, or the mac's `darwin-rebuild` breaks. Renaming the output attr
  is a breaking change for the consumer; the repo name is not (GitHub redirects).
- The per-tool `config.toml`s under `crates/*/` are env-specific and were vendored
  into `nixos-config` (`modules/features/dev-tools/*.toml`); they are no longer read
  from this repo.
- After any flake change, validate: `nix flake check` + `nix build
  .#packages.aarch64-darwin.dev-tools`, and ideally re-`nix flake update dev-tools`
  in `~/nixos-config` + rebuild.

## Releasing

One `v<version>` tag → one GitHub Release; each tool ships as its own
`<tool>-aarch64-apple-darwin.tar.gz`. One version covers all tools.

```sh
git tag v0.2.0
git push origin v0.2.0
```

`.github/workflows/release.yml` triggers on `v*`, builds the workspace on a
`macos-14`/`aarch64-apple-darwin` runner, and packages every `[[bin]]` crate under
`crates/`. New tools need no workflow edits. Releases are built only for
`aarch64-apple-darwin`; other platforms build locally.

## Environment gotchas (Claude's Bash sandbox)

- `gh`/`nix` are not on Claude's Bash PATH by default — prepend
  `export PATH="/etc/profiles/per-user/itayka/bin:$PATH"`.
- Pushing needs an account switch: `gh auth switch --user Itaykal` (then switch back
  to `itayka_dream`).
- Nix builds need a token to avoid GitHub rate limits:
  `export NIX_CONFIG="access-tokens = github.com=$(gh auth token)"`. The first build
  pulls the full Rust toolchain (~15 min) — run it in the background.
- Pushing a commit/tag that changes `.github/workflows/*` needs a token (or SSH key)
  with the `workflow` scope.
