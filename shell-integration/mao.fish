# Mao-Term shell integration for fish. Loaded with `fish --init-command 'source <this file>'`.
# Emits OSC 133 (prompt / command marks) and OSC 7 (current directory),
# and defines `mao open <file>` (OSC 7770) to show a file in Mao-Term's viewer.
# See docs/specs/shell-integration.md. Status: untested on Linux.

if set -q __mao_term
    return
end
set -g __mao_term 1

function __mao_postexec --on-event fish_postexec
    printf '\e]133;D;%s\a' $status
end

function __mao_preexec --on-event fish_preexec
    printf '\e]133;C\a'
end

function __mao_prompt_start --on-event fish_prompt
    printf '\e]7;file://%s%s\a' (hostname) (string escape --style=url -- $PWD)
    printf '\e]133;A\a'
end

if functions -q fish_prompt
    functions -c fish_prompt __mao_user_prompt
    function fish_prompt
        __mao_user_prompt
        printf '\e]133;B\a'
    end
end

# `mao open <file>`: show a file in Mao-Term's document viewer.
function mao
    if test (count $argv) -lt 2; or test "$argv[1]" != open
        echo 'usage: mao open <file>'
        return 2
    end
    set -l full (realpath -- $argv[2]); or return 1
    printf '\e]7770;open;file://%s%s\a' (hostname) (string escape --style=url -- $full)
end
