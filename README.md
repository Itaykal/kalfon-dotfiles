# kalfon-dotfiles

Personal zsh dotfiles for macOS — AWS SSO account switching, Powerlevel10k overrides, and shell aliases.

## Structure

The root holds only the entry points (`.entry`, `install.sh`, `Brewfile`) and docs. Everything else lives in one of three buckets by concern — `zsh/` (sourced into the shell), `tools/` (compiled Rust CLIs), `system/` (artifacts symlinked into external locations):

```
kalfon-dotfiles/
  .entry                # dynamic loader — sources all *.zsh from each zsh/ dir in order
  install.sh            # installer
  Brewfile              # brew bundle manifest

  zsh/                  # everything .entry sources into the shell
    init/               # runs first — third-party integrations
      fzf.zsh           # fzf key-bindings (Ctrl-R/Ctrl-T/Alt-C) + fuzzy completion
    vars/               # exported environment variables only
      locale.zsh        # LC_TIME
      path.zsh          # puts tools/bin on PATH
    aliases/            # aliases only, one file per tool — no functions
      aws.zsh           # awsw
      kubernetes.zsh    # kk, kx, kn
      terraform.zsh     # tf, tg
    functions/          # shell functions, one file per function — no aliases
      aws-sync-prompt.zsh
    p10k/               # Powerlevel10k overrides, numbered for load order
      1-kubernetes.zsh  # kubecontext: placement, visibility, coloring
      2-aws.zsh         # AWS segment: placement, coloring, content format

  tools/                # Rust/Cargo workspace — compiled CLIs (aws-switch, feature) on PATH via tools/bin

  system/               # external app/OS artifacts symlinked out by install.sh
    k9s/                # symlinked into ~/Library/Application Support/k9s/
      views.yaml        # enriched default columns for pods, deploys, sts, nodes, …
    macos/              # macOS LaunchAgents / AppleScripts
```

Load order is `init → vars → aliases → functions → p10k`. `zsh/init/` runs first so anything later (vars, aliases, functions, prompt) can rely on third-party widgets and integrations already being in place.

## Installation

```zsh
git clone https://github.com/Itaykal/kalfon-dotfiles.git ~/kalfon-dotfiles
~/kalfon-dotfiles/install.sh
```

`install.sh` handles everything:

- installs [oh-my-zsh](https://ohmyz.sh/) to `~/.oh-my-zsh` if missing
- clones [Powerlevel10k](https://github.com/romkatv/powerlevel10k) to `~/powerlevel10k` if missing
- runs `brew bundle` against [`Brewfile`](./Brewfile) (fzf, jq, direnv, k9s, kubectx, awscli)
- symlinks `system/k9s/views.yaml` into `~/Library/Application Support/k9s/`
- appends `source ~/kalfon-dotfiles/.entry` to `~/.zshrc` if not already there

After install: `exec zsh`. Then `p10k configure` if first time on this machine. Configure AWS SSO with a session named `session` (`aws configure sso`).

## AWS SSO Switcher (`awsw`)

Run `awsw` to switch AWS accounts:

1. Checks if the SSO session is valid; re-authenticates via `aws sso login --sso-session session` if expired.
2. Lists all accounts available to you in the SSO portal (real account names, not profile keys).
3. If the selected account has multiple roles, prompts to pick one.
4. Writes the selection into a single `[profile default]` in `~/.aws/config` — no profile sprawl.
5. Exports `AWS_PROFILE`, `AWS_ACCOUNT_ID`, `AWS_ACCOUNT_NAME`, `AWS_ROLE_NAME`, and `AWS_DEFAULT_REGION`.

The Powerlevel10k prompt always shows the active account: `account-name  account-id  role  region`.

## Powerlevel10k overrides (`zsh/p10k/`)

- **kubecontext** (`1-kubernetes.zsh`) — own line at the top-left, above `dir/vcs`. Visible only when typing k8s commands.
- **aws** (`2-aws.zsh`) — own line below kubecontext, above `dir/vcs`. Always visible.

Files are numbered because the AWS block inserts relative to positions set by the kubernetes block — load order matters. These overrides survive `p10k configure` regenerating `~/.p10k.zsh` because they live in a separate file.
