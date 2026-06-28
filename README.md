# dev-tools

Personal terminal CLIs, written in Rust. Each tool is a self-contained binary;
the repo is a single Cargo workspace exposed as a Nix flake.

| Tool | What it does |
|------|--------------|
| `aws-switch` | Pick an AWS SSO account + role from a fuzzy list → writes a single `[profile default]` into `~/.aws/config` (no profile sprawl). Re-authenticates the SSO session when expired. |
| `feature` | Pick a Jira issue (or create one) from a fuzzy list → `git checkout -b` a branch for it. |
| `wt-gc` | Garbage-collect stale git worktrees. |

Shared UI lives in the `common` library crate (theme, fuzzy picker, config +
cache loaders, terminal guard, spinner).

## Layout

```
dev-tools/
  Cargo.toml          workspace + shared dependency versions
  rust-toolchain.toml stable channel + rustfmt/clippy
  Makefile            build / dev / test / fmt / clippy
  flake.nix           Nix flake — packages.<system>.dev-tools builds all binaries
  crates/
    common/           shared lib (no binary)
    aws-switch/        [[bin]]
    feature/           [[bin]]
    wt-gc/             [[bin]]
```

## Consumption (Nix)

The mac is managed by [`Itaykal/nixos-config`](https://github.com/Itaykal/nixos-config),
which takes this repo as the `dev-tools` flake input and installs the binaries via
`inputs.dev-tools.packages.<system>.dev-tools`. The per-tool `config.toml`s are
vendored in `nixos-config` (env-specific), not read from here.

## Local development

```sh
nix develop            # shell with the pinned Rust toolchain + rust-analyzer
make dev               # fast debug build, symlinks binaries into bin/
make test              # cargo test across the workspace
make clippy            # cargo clippy -D warnings
```

MSRV is `1.96` (newer than nixpkgs 26.05's default rustc), so the flake pins the
toolchain via `rust-overlay`.

## Releasing

One tag → one GitHub Release, with a standalone tarball per tool. One version
covers all tools.

```sh
git tag v0.2.0
git push origin v0.2.0
```

`.github/workflows/release.yml` triggers on `v*` tags, builds the workspace on an
`aarch64-apple-darwin` runner, and packages **every** `[[bin]]` crate under
`crates/` into its own `<tool>-aarch64-apple-darwin.tar.gz`. Library crates (like
`common`) are skipped automatically — **new tools need no workflow edits**: add a
`[[bin]]` crate under `crates/` and CI + the flake pick it up.

Releases are built only for `aarch64-apple-darwin`; other platforms build locally.
