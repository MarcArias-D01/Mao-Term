# Mao-Term shell integration for bash. Loaded with `bash --rcfile <this file>`.
# Emits OSC 133 (prompt / command marks) and OSC 7 (current directory),
# and defines `mao open <file>` (OSC 7770) to show a file in Mao-Term's viewer.
# See docs/specs/shell-integration.md. Status: untested on Linux.

if [ -n "${__MAO_TERM-}" ]; then return; fi
__MAO_TERM=1

# --rcfile replaces ~/.bashrc, so load the user's config first.
if [ -f "$HOME/.bashrc" ]; then . "$HOME/.bashrc"; fi

__mao_urlencode() {
    local LC_ALL=C s="$1" out="" c i
    for ((i = 0; i < ${#s}; i++)); do
        c=${s:i:1}
        case "$c" in
            [a-zA-Z0-9/._~-]) out+="$c" ;;
            *) out+=$(printf '%%%02X' "'$c") ;;
        esac
    done
    printf '%s' "$out"
}

__mao_precmd() {
    local code=$?
    if [ -n "${__mao_ran-}" ]; then printf '\e]133;D;%s\a' "$code"; fi
    __mao_ran=1
    printf '\e]7;file://%s%s\a' "${HOSTNAME-}" "$(__mao_urlencode "$PWD")"
    return $code
}

# Re-wrap PS1 every time: prompt frameworks may rebuild it in PROMPT_COMMAND.
__mao_wrap_ps1() {
    case "$PS1" in
        *'133;B'*) ;;
        *) PS1='\[\e]133;A\a\]'"$PS1"'\[\e]133;B\a\]' ;;
    esac
}

PROMPT_COMMAND="__mao_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND};__mao_wrap_ps1"

# `mao open <file>`: show a file in Mao-Term's document viewer.
mao() {
    if [ "${1-}" != open ] || [ -z "${2-}" ]; then echo 'usage: mao open <file>'; return 2; fi
    if [ ! -e "$2" ]; then echo "mao: no such file: $2" >&2; return 1; fi
    local dir
    dir=$(cd -- "$(dirname -- "$2")" && pwd) || return 1
    printf '\e]7770;open;file://%s%s\a' "${HOSTNAME-}" "$(__mao_urlencode "$dir/$(basename -- "$2")")"
}
