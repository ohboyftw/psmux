# Shared helper library for psmux demo recordings
# Source this from each demo script: . "$PSScriptRoot/lib-demo.ps1"

$script:CaptionLog = @()
$script:CaptionIndex = 0
$script:StartTime = $null

function Demo-Init {
    param([string]$SessionName = "demo")
    $script:StartTime = [DateTimeOffset]::UtcNow
    $script:CaptionLog = @()
    $script:CaptionIndex = 0
    $script:Session = $SessionName

    # Kill any existing demo session
    psmux kill-session -t $SessionName 2>$null
    Start-Sleep -Milliseconds 500
}

function Demo-Caption {
    param([string]$Text)
    $script:CaptionIndex++
    $elapsed = ([DateTimeOffset]::UtcNow - $script:StartTime).TotalSeconds
    $script:CaptionLog += [PSCustomObject]@{
        Index = $script:CaptionIndex
        Start = $elapsed
        End   = $elapsed + 3.0  # default 3s display
        Text  = $Text
    }
}

function Demo-Send {
    # Send keys to the demo session target pane, with caption
    param(
        [string]$Keys,
        [string]$Caption = "",
        [string]$Target = "",
        [int]$WaitMs = 800
    )
    $t = if ($Target) { "-t $Target" } else { "-t $script:Session" }
    $cmd = "psmux send-keys $t $Keys"
    Invoke-Expression $cmd
    if ($Caption) { Demo-Caption $Caption }
    Start-Sleep -Milliseconds $WaitMs
}

function Demo-Type {
    # Type text followed by Enter, with caption showing the command
    param(
        [string]$Text,
        [string]$Caption = "",
        [string]$Target = "",
        [int]$WaitMs = 1200
    )
    $t = if ($Target) { $Target } else { $script:Session }
    psmux send-keys -t $t "$Text" Enter
    $cap = if ($Caption) { $Caption } else { "$ $Text" }
    Demo-Caption $cap
    Start-Sleep -Milliseconds $WaitMs
}

function Demo-Prefix {
    # Send Ctrl+b followed by a key
    param(
        [string]$Key,
        [string]$Caption = "",
        [int]$WaitMs = 1000
    )
    psmux send-keys -t $script:Session C-b
    Start-Sleep -Milliseconds 200
    psmux send-keys -t $script:Session $Key
    if ($Caption) { Demo-Caption $Caption }
    Start-Sleep -Milliseconds $WaitMs
}

function Demo-Wait {
    param([int]$Ms = 1500)
    Start-Sleep -Milliseconds $Ms
}

function Demo-SaveCaptions {
    param([string]$Path)
    # Write SRT subtitle file
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
    # Attach to the session — this is what PowerSession records
    param([int]$DurationMs = 0)
    if ($DurationMs -gt 0) {
        # Auto-detach after duration (for automated recording)
        Start-Job -ScriptBlock {
            param($ms, $session)
            Start-Sleep -Milliseconds $ms
            psmux detach-client -t $session 2>$null
        } -ArgumentList $DurationMs, $script:Session | Out-Null
    }
    psmux attach -t $script:Session
}
