# dev-tools — agent guide

Personal terminal CLIs in Rust, a single Cargo workspace exposed as a Nix flake
(`aws-switch`, `feature`, `wt-gc`; shared code in `common`).

See **`CLAUDE.md`** for the full conventions — structure, the Nix flake + consumer
contract (`Itaykal/nixos-config` depends on `packages.<system>.dev-tools`), the
release flow, and the Bash-sandbox environment gotchas (gh/nix PATH, push account,
nix token).

Quick rules:
- New tool → `crates/<tool>/` with a `[[bin]]`; CI + the flake auto-discover it.
- Shared code → `crates/common/`.
- `cargo fmt` + `cargo clippy --all-targets -- -D warnings` before committing; keep
  `Cargo.lock` committed (CI builds `--locked`).
- Release = push a `v<version>` tag.
