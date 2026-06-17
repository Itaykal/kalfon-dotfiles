# kalfon-dotfiles — agent guide

Personal zsh dotfiles repo. `CLAUDE.md` holds the full repo conventions (structure,
shell layout rules, p10k/k9s/macos notes) — read it first. This file documents the
release/versioning flow for the Rust tools.

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
Silicon macOS, downloads each tool's tarball from the latest release
(`releases/latest/download/<tool>-aarch64-apple-darwin.tar.gz`) into `tools/bin/`.
If any download fails, it falls back to building everything from source via
`make -C tools build`.

Caveats:
- Releases are built only for `aarch64-apple-darwin`; other platforms always build locally.
- Pushing a commit/tag that adds or changes `.github/workflows/*` requires a GitHub token
  (or SSH key) with the `workflow` scope.
