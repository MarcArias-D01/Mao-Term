# Mao-Term: Mao-Term sets ZDOTDIR to this folder so its .zshrc runs.
# Load the user's .zshenv without changing ZDOTDIR yet. Status: untested on Linux.
if [[ -f "${MAO_USER_ZDOTDIR:-$HOME}/.zshenv" ]]; then
    source "${MAO_USER_ZDOTDIR:-$HOME}/.zshenv"
fi
