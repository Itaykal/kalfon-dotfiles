# AWS: inline with kubecontext on the top line, followed by a newline, then dir/vcs.
# Depends on 1-kubernetes.zsh having run first (kubecontext at [1]).
if [[ ${POWERLEVEL9K_LEFT_PROMPT_ELEMENTS[2]} != aws ]]; then
  typeset -g POWERLEVEL9K_LEFT_PROMPT_ELEMENTS=(
    ${POWERLEVEL9K_LEFT_PROMPT_ELEMENTS[1]}
    aws
    newline
    ${POWERLEVEL9K_LEFT_PROMPT_ELEMENTS[2,-1]}
  )
fi
typeset -g POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS=(${POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS[@]:#aws})

unset POWERLEVEL9K_AWS_SHOW_ON_COMMAND

# Class matching runs against AWS_VAULT (set to account name by aws-switch).
# (#i) makes the glob case-insensitive so *prod* catches cti-prod, ProdAlbOra, etc.
typeset -g POWERLEVEL9K_AWS_CLASSES=(
  '(#i)*prod*'     PROD
  '(#i)*critical*' PROD
  '*'              DEFAULT)
typeset -g POWERLEVEL9K_AWS_PROD_FOREGROUND=196     # red
typeset -g POWERLEVEL9K_AWS_DEFAULT_FOREGROUND=208  # orange

# Format: (account-name | account-id | role | region)
# P9K_AWS_PROFILE resolves to AWS_VAULT (account name) when set.
typeset -g POWERLEVEL9K_AWS_CONTENT_EXPANSION='(${P9K_AWS_PROFILE//\%/%%}${AWS_ACCOUNT_ID:+ | ${AWS_ACCOUNT_ID//\%/%%}}${AWS_ROLE_NAME:+ | ${AWS_ROLE_NAME//\%/%%}}${P9K_AWS_REGION:+ | ${P9K_AWS_REGION//\%/%%}})'

(( $+functions[p10k] )) && p10k reload
