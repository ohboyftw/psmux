<#
.SYNOPSIS
    Connects to psmux named pipe and monitors push events for agent health.
.DESCRIPTION
    Consumes context_ready, context_exited, and exec_completed events.
    Detects stalls (no events from a pane within threshold) and failures
    (non-zero exit codes). Prints structured log lines.
.PARAMETER Session
    psmux session name to monitor. Default: "default"
.PARAMETER StallThresholdSeconds
    Seconds of silence before reporting a stall. Default: 300
#>
param(
    [string]$Session = "default",
    [int]$StallThresholdSeconds = 300,
    [switch]$Json
)

$pipePath = "\\.\pipe\psmux-claude-backend-$Session"
$paneState = @{}  # context_id -> @{last_event_time, status, command}

function Log($level, $msg, $data) {
    $ts = Get-Date -Format "yyyy-MM-dd HH:mm:ss"
    if ($Json) {
        $obj = @{ timestamp = $ts; level = $level; message = $msg }
        if ($data) { $obj.data = $data }
        $obj | ConvertTo-Json -Compress
    } else {
        Write-Host "[$ts] [$level] $msg" -ForegroundColor $(
            switch ($level) { "ERROR" { "Red" } "WARN" { "Yellow" } default { "Gray" } }
        )
    }
}

try {
    Log "INFO" "Connecting to pipe: $pipePath"

    $pipe = [System.IO.Pipes.NamedPipeClientStream]::new(".", "psmux-claude-backend-$Session", [System.IO.Pipes.PipeDirection]::InOut)
    $pipe.Connect(5000)
    $reader = [System.IO.StreamReader]::new($pipe)

    Log "INFO" "Connected. Monitoring events..."

    while ($true) {
        $line = $reader.ReadLine()
        if ($null -eq $line) { break }

        try {
            $event = $line | ConvertFrom-Json
        } catch {
            continue  # Skip non-JSON lines (RPC responses)
        }

        $method = $event.method
        if (-not $method) { continue }

        $cid = $event.params.context_id
        $now = Get-Date

        switch ($method) {
            "context_ready" {
                $paneState[$cid] = @{ last_event_time = $now; status = "ready" }
                Log "INFO" "Pane $cid ready (data_version=$($event.params.data_version))"
            }
            "context_exited" {
                $code = $event.params.exit_code
                $elapsed = $event.params.elapsed_ms
                $cmd = $event.params.command
                $paneState[$cid] = @{ last_event_time = $now; status = "exited"; exit_code = $code }

                if ($code -and $code -ne 0) {
                    Log "ERROR" "Pane $cid FAILED (exit_code=$code, command=$cmd, elapsed=${elapsed}ms)"
                } else {
                    Log "INFO" "Pane $cid exited OK (elapsed=${elapsed}ms, command=$cmd)"
                }
            }
            "exec_completed" {
                $code = $event.params.exit_code
                $paneState[$cid] = @{ last_event_time = $now; status = "exec_done" }
                if ($code -ne 0) {
                    Log "WARN" "exec in $cid failed (exit_code=$code, command=$($event.params.command))"
                }
            }
        }

        # Stall detection: check all tracked panes
        foreach ($id in @($paneState.Keys)) {
            $state = $paneState[$id]
            if ($state.status -notin @("exited", "exec_done")) {
                $silent = ($now - $state.last_event_time).TotalSeconds
                if ($silent -gt $StallThresholdSeconds) {
                    Log "WARN" "Pane $id may be stalled (${silent}s since last event)"
                    $paneState[$id].last_event_time = $now  # Reset to avoid repeated warnings
                }
            }
        }
    }
} catch {
    Log "ERROR" "Pipe connection failed: $_"
    exit 1
} finally {
    if ($reader) { $reader.Dispose() }
    if ($pipe) { $pipe.Dispose() }
}
