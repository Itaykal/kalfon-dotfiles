# kalfon-dotfiles

Personal zsh dotfiles for macOS — AWS SSO account switching, Powerlevel10k overrides, and shell aliases.

## Structure

```
kalfon-dotfiles/
  .entry              # dynamic loader — sources all *.zsh from each dir in order
  init/               # initialization scripts that must run before everything else (third-party integrations, etc.)
    fzf.zsh           # fzf key-bindings (Ctrl-R/Ctrl-T/Alt-C) + fuzzy completion
  vars/               # exported environment variables only
    locale.zsh        # LC_TIME
  aliases/            # aliases only, one file per tool — no functions
    aws.zsh           # awsw
    kubernetes.zsh    # kk, kx, kn
    terraform.zsh     # tf, tg
  functions/          # shell functions, one file per function — no aliases
    aws-switch.zsh    # aws-switch function
  p10k/               # Powerlevel10k overrides, numbered for load order
    1-kubernetes.zsh  # kubecontext: placement, visibility, coloring
    2-aws.zsh         # AWS segment: placement, coloring, content format
```

Load order is `init → vars → aliases → functions → p10k`. `init/` runs first so anything later (vars, aliases, functions, prompt) can rely on third-party widgets and integrations already being in place.

## Installation

```zsh
git clone https://github.com/Itaykal/kalfon-dotfiles.git ~/kalfon-dotfiles
```

Add to `~/.zshrc` (after oh-my-zsh and p10k are loaded):

```zsh
source ~/kalfon-dotfiles/.entry
```

### Dependencies

- [oh-my-zsh](https://ohmyz.sh/)
- [Powerlevel10k](https://github.com/romkatv/powerlevel10k)
- [fzf](https://github.com/junegunn/fzf) — `brew install fzf`
- [jq](https://jqlang.github.io/jq/) — `brew install jq`
- [k9s](https://k9scli.io/), [kubectx/kubens](https://github.com/ahmetb/kubectx) — for k8s aliases
- AWS CLI v2 configured with an SSO session named `session`

## AWS SSO Switcher (`awsw`)

Run `awsw` to switch AWS accounts:

1. Checks if the SSO session is valid; re-authenticates via `aws sso login --sso-session session` if expired.
2. Lists all accounts available to you in the SSO portal (real account names, not profile keys).
3. If the selected account has multiple roles, prompts to pick one.
4. Writes the selection into a single `[profile default]` in `~/.aws/config` — no profile sprawl.
5. Exports `AWS_PROFILE`, `AWS_ACCOUNT_ID`, `AWS_ACCOUNT_NAME`, `AWS_ROLE_NAME`, and `AWS_DEFAULT_REGION`.

The Powerlevel10k prompt always shows the active account: `account-name  account-id  role  region`.

## Powerlevel10k overrides (`p10k/`)

- **kubecontext** (`1-kubernetes.zsh`) — own line at the top-left, above `dir/vcs`. Visible only when typing k8s commands.
- **aws** (`2-aws.zsh`) — own line below kubecontext, above `dir/vcs`. Always visible.

Files are numbered because the AWS block inserts relative to positions set by the kubernetes block — load order matters. These overrides survive `p10k configure` regenerating `~/.p10k.zsh` because they live in a separate file.
