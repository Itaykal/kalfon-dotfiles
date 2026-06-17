# Kubernetes: inline with aws on the top line; hide duplicate on the right prompt.
# Show only while typing common k8s CLI commands.
if [[ ${POWERLEVEL9K_LEFT_PROMPT_ELEMENTS[1]} != kubecontext ]]; then
  typeset -g POWERLEVEL9K_LEFT_PROMPT_ELEMENTS=(
    kubecontext
    ${POWERLEVEL9K_LEFT_PROMPT_ELEMENTS[@]}
  )
fi
if (( ${POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS[(I)kubecontext]} )); then
  typeset -g POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS=(${POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS[@]:#kubecontext})
fi
typeset -g POWERLEVEL9K_KUBECONTEXT_SHOW_ON_COMMAND='kubectl|helm|kubens|kubectx|oc|istioctl|kogito|k9s|helmfile|flux|fluxctl|stern|kubeseal|skaffold|kubent|kubecolor|cmctl|sparkctl'

# Conditional coloring based on context name. Add your prod/critical patterns before the '*' catch-all.
typeset -g POWERLEVEL9K_KUBECONTEXT_CLASSES=(
  # '(#i)*prod*'  PROD    # example: add patterns here
  # '(#i)*live*'  PROD
  '*'             DEFAULT)
typeset -g POWERLEVEL9K_KUBECONTEXT_PROD_FOREGROUND=196    # red    — used when class is PROD
typeset -g POWERLEVEL9K_KUBECONTEXT_DEFAULT_FOREGROUND=76   # green  — default
