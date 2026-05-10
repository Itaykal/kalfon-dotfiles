#!/usr/bin/env zsh
# kalfon-dotfiles installer.
#   - installs oh-my-zsh (~/.oh-my-zsh) if missing
#   - clones powerlevel10k to ~/powerlevel10k if missing
#   - brew bundle from ./Brewfile
#   - symlinks k9s/views.yaml into ~/Library/Application Support/k9s/
#   - ensures ~/.zshrc sources .entry

set -euo pipefail

REPO_DIR="${0:A:h}"
K9S_CFG="$HOME/Library/Application Support/k9s"
P10K_DIR="$HOME/powerlevel10k"
OMZ_DIR="$HOME/.oh-my-zsh"
ZSHRC="$HOME/.zshrc"
ENTRY="$REPO_DIR/.entry"

echo "==> oh-my-zsh"
if [[ -d "$OMZ_DIR" ]]; then
  echo "    already installed"
else
  RUNZSH=no KEEP_ZSHRC=yes sh -c "$(curl -fsSL https://raw.githubusercontent.com/ohmyzsh/ohmyzsh/master/tools/install.sh)"
fi

echo "==> powerlevel10k"
if [[ -d "$P10K_DIR" ]]; then
  echo "    already cloned ($P10K_DIR)"
else
  git clone --depth=1 https://github.com/romkatv/powerlevel10k.git "$P10K_DIR"
fi

echo "==> brew bundle"
if ! command -v brew >/dev/null 2>&1; then
  echo "Homebrew not found. Install from https://brew.sh first." >&2
  exit 1
fi
brew bundle --file="$REPO_DIR/Brewfile"

echo "==> linking k9s/views.yaml"
mkdir -p "$K9S_CFG"
target="$K9S_CFG/views.yaml"
src="$REPO_DIR/k9s/views.yaml"
if [[ -L "$target" ]]; then
  current="$(readlink "$target")"
  if [[ "$current" == "$src" ]]; then
    echo "    already linked"
  else
    echo "    relinking (was -> $current)"
    ln -sfn "$src" "$target"
  fi
elif [[ -e "$target" ]]; then
  backup="$target.backup.$(date +%Y%m%d%H%M%S)"
  echo "    existing file found, backing up to $backup"
  mv "$target" "$backup"
  ln -s "$src" "$target"
else
  ln -s "$src" "$target"
fi

echo "==> ~/.zshrc"
if [[ -f "$ZSHRC" ]] && grep -qF "$ENTRY" "$ZSHRC"; then
  echo "    .entry already sourced"
else
  {
    echo ""
    echo "# kalfon-dotfiles"
    echo "source $ENTRY"
  } >> "$ZSHRC"
  echo "    appended source line"
fi

echo
echo "Done. Open a new shell (or 'exec zsh') to pick up the changes."
echo "If p10k hasn't been configured on this machine yet, run: p10k configure"
