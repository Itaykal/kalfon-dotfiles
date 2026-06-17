# Put the repo's compiled Rust tools on PATH so the standalone CLIs (aws-switch,
# …) resolve as plain commands. Binaries are built into tools/bin by
# `make -C tools build` (run by install.sh). %x is this file; the repo root
# is three levels up from zsh/vars/.
typeset -g _kdf_root="${${(%):-%x}:A:h:h:h}"
export PATH="$_kdf_root/tools/bin:$PATH"
unset _kdf_root
