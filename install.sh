#!/usr/bin/env zsh
# kalfon-dotfiles installer.
#   - installs oh-my-zsh (~/.oh-my-zsh) if missing
#   - clones powerlevel10k to ~/powerlevel10k if missing
#   - brew bundle from ./Brewfile
#   - symlinks system/k9s/views.yaml into ~/Library/Application Support/k9s/
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

echo "==> rust tools"
TOOLS_REPO="Itaykal/kalfon-dotfiles"
TOOLS_DIR="$REPO_DIR/tools"
TOOLS_BIN="$TOOLS_DIR/bin"
TOOLS_PLATFORM="aarch64-apple-darwin"

# Download a tool's asset from the latest release into tools/bin/. The repo is
# private, so this goes through `gh` (authenticated); a fresh/unauthenticated
# machine falls back to building from source below.
install_prebuilt_tool() {
  local tool="$1"
  local asset="$tool-$TOOLS_PLATFORM.tar.gz"
  local tmp; tmp="$(mktemp -d)"
  if gh release download --repo "$TOOLS_REPO" --pattern "$asset" --dir "$tmp" --clobber >/dev/null 2>&1; then
    mkdir -p "$TOOLS_BIN"
    tar -xzf "$tmp/$asset" -C "$TOOLS_BIN"
    chmod +x "$TOOLS_BIN/$tool"
    rm -rf "$tmp"
    echo "    installed $tool from the latest release"
    return 0
  fi
  rm -rf "$tmp"
  return 1
}

build_tools_locally() {
  if command -v cargo >/dev/null 2>&1; then
    echo "    building locally"
    make -C "$TOOLS_DIR" build
  else
    echo "    cargo not found — skipping (brew bundle installs rust; open a new shell and re-run)" >&2
  fi
}

if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]] && command -v gh >/dev/null 2>&1; then
  tools_ok=1
  for manifest in "$TOOLS_DIR"/crates/*/Cargo.toml; do
    grep -q '^\[\[bin\]\]' "$manifest" || continue   # skip library crates (e.g. common)
    tool="${manifest:h:t}"                           # tools/crates/<tool>/Cargo.toml -> <tool>
    install_prebuilt_tool "$tool" || tools_ok=0
  done
  [[ "$tools_ok" == 1 ]] || build_tools_locally       # any miss (e.g. gh not authed) → build from source
else
  build_tools_locally
fi

echo "==> linking aws-switch config"
ASW_CFG_DIR="$HOME/.config/aws-switch"
mkdir -p "$ASW_CFG_DIR"
asw_target="$ASW_CFG_DIR/config.toml"
asw_src="$REPO_DIR/tools/crates/aws-switch/config.toml"
if [[ -L "$asw_target" && "$(readlink "$asw_target")" == "$asw_src" ]]; then
  echo "    already linked"
elif [[ -e "$asw_target" ]]; then
  echo "    existing $asw_target found, leaving it in place"
else
  ln -s "$asw_src" "$asw_target"
  echo "    linked $asw_target -> $asw_src"
fi

echo "==> linking feature config"
FEAT_CFG_DIR="$HOME/.config/feature"
mkdir -p "$FEAT_CFG_DIR"
feat_target="$FEAT_CFG_DIR/config.toml"
feat_src="$REPO_DIR/tools/crates/feature/config.toml"
if [[ -L "$feat_target" && "$(readlink "$feat_target")" == "$feat_src" ]]; then
  echo "    already linked"
elif [[ -e "$feat_target" ]]; then
  echo "    existing $feat_target found, leaving it in place"
else
  ln -s "$feat_src" "$feat_target"
  echo "    linked $feat_target -> $feat_src"
fi

echo "==> linking system/k9s/views.yaml"
mkdir -p "$K9S_CFG"
target="$K9S_CFG/views.yaml"
src="$REPO_DIR/system/k9s/views.yaml"
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

echo "==> Cursor merge-windows LaunchAgent"
AGENT_LABEL="com.itayka.cursor-merge-windows"
AGENT_SRC="$REPO_DIR/system/macos/cursor-merge-windows/$AGENT_LABEL.plist"
AGENT_DIR="$HOME/Library/LaunchAgents"
AGENT_DST="$AGENT_DIR/$AGENT_LABEL.plist"
mkdir -p "$AGENT_DIR"
if [[ -L "$AGENT_DST" && "$(readlink "$AGENT_DST")" == "$AGENT_SRC" ]]; then
  echo "    already linked"
else
  ln -sfn "$AGENT_SRC" "$AGENT_DST"
  echo "    linked $AGENT_DST -> $AGENT_SRC"
fi
DOMAIN="gui/$(id -u)"
launchctl bootout "$DOMAIN/$AGENT_LABEL" 2>/dev/null || true
launchctl bootstrap "$DOMAIN" "$AGENT_DST"
echo "    loaded (first run will prompt for Accessibility permission for osascript)"

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
