# Re-read [profile default] from ~/.aws/config before each prompt so all terminals
# reflect the active account, even when aws-switch was run in a different shell.
function _aws_sync_prompt() {
  [[ -r ~/.aws/config ]] || return
  local line key val in_section=0
  local name='' id='' role='' region=''
  while IFS= read -r line; do
    if [[ $line == '[default]' ]]; then
      in_section=1
      continue
    fi
    if (( in_section )); then
      [[ $line == \[*\] ]] && break
      [[ $line == *=* ]] || continue
      key=${${line%%=*}// /}
      val=${${line#*=}## }
      val=${val%% }
      case $key in
        sso_account_name) name=$val ;;
        sso_account_id)   id=$val ;;
        sso_role_name)    role=$val ;;
        region)           region=$val ;;
      esac
    fi
  done < ~/.aws/config

  export AWS_PROFILE=default
  export AWS_VAULT=$name
  export AWS_ACCOUNT_ID=$id
  export AWS_ROLE_NAME=$role
  export AWS_DEFAULT_REGION=$region
}

autoload -Uz add-zsh-hook
add-zsh-hook precmd _aws_sync_prompt
