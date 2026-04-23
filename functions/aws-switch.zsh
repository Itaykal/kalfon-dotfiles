# Re-auth via the existing "session" SSO session if needed, pick a real account
# from the portal with fzf, then write it into [profile default] and export env vars.
function aws-switch() {
  if ! aws sts get-caller-identity --profile default &>/dev/null; then
    echo "SSO session expired — logging in..."
    aws sso login --sso-session session || return 1
  fi

  local sso_region
  sso_region=$(awk '/\[sso-session session\]/{f=1} f && /^sso_region/{print $3; exit}' ~/.aws/config)
  sso_region=${sso_region:-eu-west-1}

  local token
  token=$(jq -rs '[.[] | select(.accessToken != null)] | sort_by(.expiresAt) | last | .accessToken' \
    ~/.aws/sso/cache/*.json 2>/dev/null)
  [[ -z "$token" ]] && { echo "Could not read SSO token" >&2; return 1; }

  # Aligned two-column list: account name (fixed 35 chars) + account ID
  local selection
  selection=$(aws sso list-accounts \
    --access-token "$token" \
    --region "$sso_region" \
    --output json 2>/dev/null \
    | jq -r '.accountList[] | [.accountName, .accountId] | @tsv' \
    | sort \
    | awk 'BEGIN{FS="\t"} {printf "%-35s %s\n", $1, $2}' \
    | fzf --prompt="AWS account: " --height=40% --reverse) || return 1
  [[ -z "$selection" ]] && return 1

  local acct_name acct_id
  acct_id=$(awk '{print $NF}' <<< "$selection")
  acct_name=$(awk '{$NF=""; gsub(/ +$/, ""); print}' <<< "$selection")

  # Pick a role (auto-select if only one)
  local roles role_name
  roles=$(aws sso list-account-roles \
    --access-token "$token" \
    --region "$sso_region" \
    --account-id "$acct_id" \
    --query 'roleList[*].roleName' \
    --output text 2>/dev/null | tr '\t' '\n' | grep -v '^$')
  if [[ $(echo "$roles" | wc -l) -gt 1 ]]; then
    role_name=$(echo "$roles" | fzf --prompt="Role: " --height=20% --reverse) || return 1
  else
    role_name="$roles"
  fi

  # Overwrite the fixed 'default' profile
  aws configure set sso_session   session     --profile default
  aws configure set sso_account_id "$acct_id"  --profile default
  aws configure set sso_role_name  "$role_name" --profile default
  aws configure set region         "$sso_region" --profile default

  export AWS_PROFILE=default
  export AWS_VAULT="$acct_name"       # p10k reads AWS_VAULT before AWS_PROFILE for display & class matching
  export AWS_ACCOUNT_ID="$acct_id"
  export AWS_ROLE_NAME="$role_name"
  export AWS_DEFAULT_REGION="$sso_region"

  echo "${acct_name}  ${acct_id}  ${role_name}  ${AWS_DEFAULT_REGION}"
}
