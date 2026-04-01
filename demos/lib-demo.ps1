# Shared helper library for psmux demo recordings
# Source this from each demo script: . "$PSScriptRoot/lib-demo.ps1"

$script:CaptionLog = @()
$script:CaptionIndex = 0
$script:StartTime = $null
$script:Session = "demo"

function Demo-Init {
    param([string]$SessionName = "demo")
    $script:StartTime = [DateTimeOffset]::UtcNow
    $script:CaptionLog = @()
    $script:CaptionIndex = 0
    $script:Session = $SessionName

    # Kill any leftover sessions
    & psmux kill-server 2>$null
    Start-Sleep -Milliseconds 1000
}

function Demo-Caption {
    param([string]$Text)
    $script:CaptionIndex++
    $elapsed = ([DateTimeOffset]::UtcNow - $script:StartTime).TotalSeconds
    $script:CaptionLog += [PSCustomObject]@{
        Index = $script:CaptionIndex
        Start = $elapsed
        End   = $elapsed + 3.0
        Text  = $Text
    }
}

function Demo-Run {
    # Run a psmux CLI command (split-window, select-pane, etc.)
    # These are direct commands, not send-keys — no quoting issues.
    param(
        [string]$Cmd,
        [string]$Caption = "",
        [int]$WaitMs = 1000
    )
    Invoke-Expression "psmux $Cmd"
    if ($Caption) { Demo-Caption $Caption }
    Start-Sleep -Milliseconds $WaitMs
}

function Demo-Keys {
    # Send literal keystrokes to a pane's shell.
    # Each argument is a separate send-keys token.
    # Use -- to prevent psmux from eating flags in the text.
    param(
        [string]$Target,
        [string[]]$Keys,
        [string]$Caption = "",
        [int]$WaitMs = 1000
    )
    $t = if ($Target) { $Target } else { $script:Session }
    foreach ($k in $Keys) {
        & psmux send-keys -t $t -l -- $k
    }
    & psmux send-keys -t $t Enter
    if ($Caption) { Demo-Caption $Caption }
    Start-Sleep -Milliseconds $WaitMs
}

function Demo-ShellCmd {
    # Type a shell command into the active pane.
    # Uses send-keys -l (literal) to avoid key-name interpretation,
    # then sends Enter separately.
    param(
        [string]$Text,
        [string]$Caption = "",
        [string]$Target = "",
        [int]$WaitMs = 1200
    )
    $t = if ($Target) { $Target } else { $script:Session }
    # -l flag treats the text as literal (no key-name parsing)
    & psmux send-keys -t $t -l -- $Text
    & psmux send-keys -t $t Enter
    $cap = if ($Caption) { $Caption } else { "$ $Text" }
    Demo-Caption $cap
    Start-Sleep -Milliseconds $WaitMs
}

function Demo-Wait {
    param([int]$Ms = 1500)
    Start-Sleep -Milliseconds $Ms
}

function Demo-SaveCaptions {
    param([string]$Path)
    $srt = ""
    foreach ($c in $script:CaptionLog) {
        $startTs = [TimeSpan]::FromSeconds($c.Start).ToString("hh\:mm\:ss\,fff")
        $endTs   = [TimeSpan]::FromSeconds($c.End).ToString("hh\:mm\:ss\,fff")
        $srt += "$($c.Index)`r`n"
        $srt += "$startTs --> $endTs`r`n"
        $srt += "$($c.Text)`r`n"
        $srt += "`r`n"
    }
    Set-Content -Path $Path -Value $srt -Encoding UTF8
    Write-Host "  Captions: $Path ($($script:CaptionLog.Count) entries)" -ForegroundColor DarkGray
}

function Demo-Attach {
    param([string]$Target = "")
    $t = if ($Target) { $Target } else { $script:Session }
    Write-Host "Attaching to $t (Ctrl+b d to stop recording)..." -ForegroundColor Cyan
    Start-Sleep -Milliseconds 500
    & psmux attach -t $t
}
