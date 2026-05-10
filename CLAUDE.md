# kalfon-dotfiles

Personal zsh dotfiles repo. All shell customisations live here — never edit `~/.p10k.zsh`, `~/.zshrc` aliases, or similar files directly.

## Structure

```
.entry              # dynamic loader — sources all *.zsh from each dir in order
init/               # initialization scripts that must run before everything else (third-party integrations: fzf, zoxide, direnv, …)
vars/               # exported env vars
aliases/            # aliases only, one file per tool (no functions)
functions/          # shell functions, one file per function
p10k/               # Powerlevel10k overrides (files are numbered for load order)
k9s/                # k9s config (views.yaml, …) — symlinked into ~/Library/Application Support/k9s/
```

## Rules

- **All changes go in this repo.** Never patch `~/.p10k.zsh` or `~/.zshrc` directly.
- New aliases → new file `aliases/<tool>.zsh`. New functions → new file `functions/<name>.zsh`.
- Aliases and functions must not be mixed in the same file.
- `init/` is for anything that must run before the rest of the dotfiles load — typically `source` lines for third-party shell integrations (fzf key-bindings, zoxide, direnv hooks). Never put those `source` lines in `aliases/` or `functions/`.
- `.entry` dynamically sources `*.zsh` from each category dir in order: `init → vars → aliases → functions → p10k`.
- `p10k/` files are numbered (`1-`, `2-`, …) because load order matters: kubernetes layout must be set before the AWS block inserts relative to it.
- `.p10k` overrides are sourced after `~/.p10k.zsh`, so they win. Use array manipulation to reorder prompt elements rather than redefining the full array.
- The AWS SSO session is always named `session`. The active profile is always `default`. Do not introduce new profile names.
- `AWS_ACCOUNT_NAME`, `AWS_ACCOUNT_ID`, `AWS_ROLE_NAME`, `AWS_DEFAULT_REGION` are exported by `aws-switch` and referenced in the p10k content expansion — keep them in sync if either side changes.
- `jq` and `fzf` are assumed to be installed. Do not add fallbacks for them.
- `k9s/` is not sourced by `.entry` (it's not shell). Files there are symlinked into `~/Library/Application Support/k9s/` (e.g. `views.yaml`). Edit only via this repo. Bad column expressions in `views.yaml` fail silently in the UI — check `~/Library/Application Support/k9s/k9s.log` after changes.
