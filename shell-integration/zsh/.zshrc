# Mao-Term shell integration for zsh (loaded via ZDOTDIR).
# Emits OSC 133 (prompt / command marks) and OSC 7 (current directory).
# See docs/specs/shell-integration.md. Status: untested on Linux.

# Restore the user's ZDOTDIR and load their config.
ZDOTDIR="${MAO_USER_ZDOTDIR:-$HOME}"
unset MAO_USER_ZDOTDIR
if [[ -f "$ZDOTDIR/.zshrc" ]]; then
    source "$ZDOTDIR/.zshrc"
fi

if [[ -z "${__MAO_TERM-}" ]]; then
    __MAO_TERM=1

    __mao_urlencode() {
        local LC_ALL=C s="$1" out="" c i
        for (( i = 1; i <= ${#s}; i++ )); do
            c="${s[i]}"
            case "$c" in
                [a-zA-Z0-9/._~-]) out+="$c" ;;
                *) out+=$(printf '%%%02X' "'$c") ;;
            esac
        done
        print -rn -- "$out"
    }

    __mao_precmd() {
        local code=$?
        [[ -n "${__mao_ran-}" ]] && printf '\e]133;D;%s\a' "$code"
        __mao_ran=1
        printf '\e]7;file://%s%s\a' "$HOST" "$(__mao_urlencode "$PWD")"
    }

    # Re-wrap the prompt each time: themes may rebuild PS1 in precmd.
    __mao_wrap_prompt() {
        [[ "$PS1" == *'133;B'* ]] || PS1=$'%{\e]133;A\a%}'"$PS1"$'%{\e]133;B\a%}'
    }

    __mao_preexec() { printf '\e]133;C\a'; }

    precmd_functions=(__mao_precmd $precmd_functions __mao_wrap_prompt)
    preexec_functions+=(__mao_preexec)
fi
