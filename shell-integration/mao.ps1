# Mao-Term shell integration for Windows PowerShell 5.1 and PowerShell 7+.
# Emits OSC 133 (prompt / command marks) and OSC 7 (current directory),
# and defines `mao open <file>` (OSC 7770) to show a file in Mao-Term's viewer.
# See docs/specs/shell-integration.md. Loaded automatically by Mao-Term; safe to load twice.

if ($global:__MaoTerm) { return }
$global:__MaoTerm = @{ Prompt = $function:prompt; Ran = $false }

function global:prompt {
    # Must be the first statement: status of the last command.
    $ok = $?
    $code = $global:LASTEXITCODE
    $e = [char]27
    $bel = [char]7
    $sb = [System.Text.StringBuilder]::new()

    if ($global:__MaoTerm.Ran) {
        $exit = if ($ok) { 0 } elseif ($code) { $code } else { 1 }
        [void]$sb.Append("$e]133;D;$exit$bel")
    }
    $global:__MaoTerm.Ran = $true

    $loc = $executionContext.SessionState.Path.CurrentLocation
    if ($loc.Provider.Name -eq 'FileSystem') {
        $uri = ([System.Uri]::new($loc.ProviderPath)).AbsoluteUri
        [void]$sb.Append("$e]7;$uri$bel")
    }

    [void]$sb.Append("$e]133;A$bel")
    [void]$sb.Append((& $global:__MaoTerm.Prompt))
    [void]$sb.Append("$e]133;B$bel")

    # Don't let the prompt itself change the visible exit code.
    $global:LASTEXITCODE = $code
    return $sb.ToString()
}

# `mao open <file>`: show a file in Mao-Term's document viewer.
function global:mao {
    param([Parameter(Position = 0)][string]$Command, [Parameter(Position = 1)][string]$Path)
    if ($Command -ne 'open' -or -not $Path) {
        Write-Host 'usage: mao open <file>'
        return
    }
    $full = (Resolve-Path -LiteralPath $Path -ErrorAction Stop).ProviderPath
    $uri = ([System.Uri]::new($full)).AbsoluteUri
    [Console]::Write("$([char]27)]7770;open;$uri$([char]7)")
}
