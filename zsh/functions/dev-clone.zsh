# Clone a repo into ~/dev/repos/<name> and cd into it, keeping the dev/ layout
# tidy (clones in dev/repos, worktrees in dev/worktrees). The dir name defaults
# to the repo's basename (sans .git); pass a second arg to override it.
function dev-clone() {
  if [[ -z $1 ]]; then
    echo "usage: dev-clone <git-url> [dir-name]" >&2
    return 1
  fi
  local url=$1
  local name=${2:-${${1##*/}%.git}}
  local dest="$HOME/dev/repos/$name"
  git clone "$url" "$dest" || return
  cd "$dest"
}
