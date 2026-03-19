---
name: swarm-backend-validation
description: >
  Validate psmux as a Claude Code swarm backend. Use when testing whether psmux
  can serve as the tmux backend for Claude Code's TeammateTool, verifying
  environment variables, command output formats, pane lifecycle, send-keys
  encoding, and concurrent agent spawning. Trigger on "swarm test", "backend
  validation", "TeammateTool compatibility", "test psmux with Claude Code",
  "does psmux work with swarms", or "agent spawn test".
---

# Swarm Backend Validation for psmux

## What We're Validating

Claude Code's tmux spawn backend does these things in sequence:

1. Detects tmux via `$TMUX` environment variable
2. Splits panes via `split-window`
3. Tracks panes via `list-panes` output parsing
4. Injects prompts via `send-keys`
5. Monitors panes via `list-panes` polling
6. Kills panes via `kill-pane -t <pane_id>`

Each step has specific output format expectations. We test each one.

---

## Test 1: Environment Variable Detection

Claude Code's auto-detection logic (from spawn backend source):

```
if $TMUX is set → use tmux backend
else if $TERM_PROGRAM == "iTerm.app" → use iterm2
else if `which tmux` succeeds → use tmux (external session)
else → use in-process
```

### What to verify:

```powershell
# Start a psmux session
psmux new-session -s test-env -d

# Attach and check environment inside the session
psmux send-keys -t test-env "echo TMUX=$env:TMUX" Enter
psmux send-keys -t test-env "echo PSMUX=$env:PSMUX" Enter

# Capture the output
Start-Sleep -Seconds 2
psmux capture-pane -t test-env -p
```

### Expected result:
- `$TMUX` or `$PSMUX` must be set to a non-empty value inside child shells
- If only `$PSMUX` is set, Claude Code won't detect it — psmux MUST also set `$TMUX`
- The value format should be: `<socket_path>,<pid>,<session_index>`
  (e.g., `/tmp/tmux-1000/default,12345,0`)

### Pass criteria:
- [ ] `$TMUX` is set inside psmux sessions
- [ ] `which tmux` resolves to the psmux binary
- [ ] Value is non-empty string

---

## Test 2: list-panes Output Format

Claude Code parses `list-panes` output to extract pane IDs, dimensions, and active state.

### What to verify:

```powershell
# Create a session with multiple panes
psmux new-session -s test-panes -d
psmux split-window -h -t test-panes
psmux split-window -v -t test-panes

# Get list-panes output
$output = psmux list-panes -t test-panes
Write-Output $output
```

### Expected format (what tmux produces):
```
0: [80x24] [history 0/2000 bytes] %0 (active)
1: [80x12] [history 0/2000 bytes] %1
2: [80x12] [history 0/2000 bytes] %2
```

Key fields Claude Code extracts:
- **Pane index**: The `0:`, `1:`, `2:` at the start
- **Pane ID**: The `%0`, `%1`, `%2` identifiers
- **Active marker**: `(active)` suffix on the focused pane

### Pass criteria:
- [ ] Each line contains a `%N` pane identifier
- [ ] Active pane is marked with `(active)`
- [ ] Pane IDs are stable across repeated calls
- [ ] Format doesn't break when many panes exist (test with 10+)

---

## Test 3: split-window Returns Pane ID

When Claude Code spawns a teammate, it needs the new pane's ID to target it later.

### What to verify:

```powershell
# Split and capture the new pane ID
psmux new-session -s test-split -d
$paneId = psmux split-window -h -t test-split -P -F "#{pane_id}"
Write-Output "New pane ID: $paneId"

# Verify we can target it
psmux send-keys -t $paneId "echo hello" Enter
```

### Pass criteria:
- [ ] `-P -F "#{pane_id}"` returns the new pane's `%N` identifier
- [ ] The returned ID can be used with `-t` in subsequent commands
- [ ] Works for both `-h` (horizontal) and `-v` (vertical) splits

---

## Test 4: send-keys Encoding

Agent prompts contain complex text. Claude Code sends them via `send-keys`.

### What to verify:

```powershell
psmux new-session -s test-keys -d

# Test 4a: Basic text + Enter
psmux send-keys -t test-keys "echo hello world" Enter

# Test 4b: Quotes inside prompt
psmux send-keys -t test-keys 'echo "quoted text"' Enter

# Test 4c: Special characters
psmux send-keys -t test-keys 'echo $HOME && echo `whoami`' Enter

# Test 4d: Literal mode
psmux send-keys -l -t test-keys "literal text without key parsing"

# Test 4e: Long prompt (agent prompts can be 500+ chars)
$longPrompt = "echo " + ("A" * 500)
psmux send-keys -t test-keys $longPrompt Enter

# Capture and verify
Start-Sleep -Seconds 3
psmux capture-pane -t test-keys -p
```

### Pass criteria:
- [ ] Basic text arrives unmodified
- [ ] Quotes pass through correctly
- [ ] `Enter` is parsed as a key press, not literal text
- [ ] `-l` flag sends text literally without key name parsing
- [ ] Long strings (500+ chars) don't get truncated or corrupted
- [ ] Special chars (`$`, backtick, `&`, `|`, `>`, `<`) pass through in `-l` mode

---

## Test 5: Pane Targeting with IDs

Claude Code stores pane IDs in team config and uses them throughout the session lifecycle.

### What to verify:

```powershell
psmux new-session -s test-target -d

# Create 3 panes (simulating 3 teammates)
$pane1 = psmux split-window -h -t test-target -P -F "#{pane_id}"
$pane2 = psmux split-window -v -t test-target -P -F "#{pane_id}"
$pane3 = psmux split-window -v -t test-target -P -F "#{pane_id}"

# Send different text to each pane by ID
psmux send-keys -t $pane1 "echo PANE1" Enter
psmux send-keys -t $pane2 "echo PANE2" Enter
psmux send-keys -t $pane3 "echo PANE3" Enter

# Verify each pane received the correct text
Start-Sleep -Seconds 2
$out1 = psmux capture-pane -t $pane1 -p
$out2 = psmux capture-pane -t $pane2 -p
$out3 = psmux capture-pane -t $pane3 -p

# Check
if ($out1 -match "PANE1") { "Pane 1: PASS" } else { "Pane 1: FAIL" }
if ($out2 -match "PANE2") { "Pane 2: PASS" } else { "Pane 2: FAIL" }
if ($out3 -match "PANE3") { "Pane 3: PASS" } else { "Pane 3: FAIL" }

# Kill specific pane by ID (simulating teammate shutdown)
psmux kill-pane -t $pane2

# Verify pane2 is gone but others remain
$remaining = psmux list-panes -t test-target
if ($remaining -notmatch $pane2) { "Kill targeting: PASS" } else { "Kill targeting: FAIL" }
```

### Pass criteria:
- [ ] Each pane receives only the text sent to its specific ID
- [ ] `kill-pane -t` removes exactly the targeted pane
- [ ] Remaining pane IDs stay valid after a pane is killed
- [ ] Pane IDs don't get reassigned after kills

---

## Test 6: Concurrent Spawning (Race Conditions)

Claude Code spawns multiple teammates rapidly. This tests for race conditions.

### What to verify:

```powershell
psmux new-session -s test-concurrent -d

# Rapid-fire 5 splits (simulating swarm spawn)
$panes = @()
for ($i = 0; $i -lt 5; $i++) {
    $id = psmux split-window -v -t test-concurrent -P -F "#{pane_id}"
    $panes += $id
}

# Verify all 5 panes exist and have unique IDs
$uniquePanes = $panes | Sort-Object -Unique
if ($uniquePanes.Count -eq 5) { "Unique IDs: PASS" } else { "Unique IDs: FAIL (got $($uniquePanes.Count))" }

# Send to all panes rapidly
for ($i = 0; $i -lt 5; $i++) {
    psmux send-keys -t $panes[$i] "echo AGENT_$i" Enter
}

# Verify no cross-contamination
Start-Sleep -Seconds 3
for ($i = 0; $i -lt 5; $i++) {
    $out = psmux capture-pane -t $panes[$i] -p
    if ($out -match "AGENT_$i") { "Agent $i: PASS" } else { "Agent $i: FAIL" }
}

# Clean up
psmux kill-session -t test-concurrent
```

### Pass criteria:
- [ ] All 5 panes created successfully
- [ ] All pane IDs are unique
- [ ] No send-keys cross-contamination between panes
- [ ] No crashes or hangs during rapid spawning
- [ ] Session cleanup works after heavy pane usage

---

## Test 7: Session Persistence (Detach/Reattach)

Swarm sessions need to survive leader detach.

### What to verify:

```powershell
# Create session with work in progress
psmux new-session -s test-persist -d
psmux split-window -h -t test-persist
psmux send-keys -t test-persist "echo BEFORE_DETACH" Enter

# List sessions to confirm running
$sessions = psmux list-sessions
if ($sessions -match "test-persist") { "Session exists: PASS" } else { "Session exists: FAIL" }

# Verify pane state survives (session is detached, panes should persist)
Start-Sleep -Seconds 2
$out = psmux capture-pane -t test-persist -p
if ($out -match "BEFORE_DETACH") { "State persists: PASS" } else { "State persists: FAIL" }

# Cleanup
psmux kill-session -t test-persist
```

### Pass criteria:
- [ ] Detached sessions stay in `list-sessions`
- [ ] Pane content is preserved across detach/reattach
- [ ] Pane IDs remain valid after reattach
- [ ] `has-session` returns correct exit codes

---

## Test 8: Format String Expansion

Claude Code uses `-P -F "#{pane_id}"` to capture pane identifiers after splits.

### What to verify:

```powershell
psmux new-session -s test-format -d

# Test various format strings
$paneId = psmux split-window -h -t test-format -P -F "#{pane_id}"
$panePid = psmux split-window -v -t test-format -P -F "#{pane_pid}"
$combined = psmux split-window -v -t test-format -P -F "#{session_name}:#{window_index}.#{pane_index}"

Write-Output "pane_id: $paneId"        # Expected: %N
Write-Output "pane_pid: $panePid"      # Expected: numeric PID
Write-Output "combined: $combined"      # Expected: test-format:0.N
```

### Pass criteria:
- [ ] `#{pane_id}` returns `%N` format
- [ ] `#{pane_pid}` returns a valid numeric PID
- [ ] `#{session_name}` returns the session name
- [ ] `#{window_index}` returns numeric index
- [ ] Compound format strings work

---

## Running All Tests

Save the test scripts to `tests/` and run them as a suite:

```powershell
# Run all validation tests
$results = @{}
$testFiles = Get-ChildItem tests/*.ps1
foreach ($test in $testFiles) {
    Write-Output "Running $($test.Name)..."
    try {
        $output = & $test.FullName
        $results[$test.Name] = @{ Status = "PASS"; Output = $output }
    } catch {
        $results[$test.Name] = @{ Status = "FAIL"; Error = $_.Exception.Message }
    }
}

# Summary
$pass = ($results.Values | Where-Object { $_.Status -eq "PASS" }).Count
$fail = ($results.Values | Where-Object { $_.Status -eq "FAIL" }).Count
Write-Output "`n=== RESULTS: $pass passed, $fail failed ==="
```

---

## What to Do With Failures

| Test | Failure Mode | Fix Required In |
|------|-------------|-----------------|
| Test 1 | `$TMUX` not set | psmux session init — must set `$TMUX` in child shell environment |
| Test 2 | Format mismatch | psmux `list-panes` output formatter — match tmux format exactly |
| Test 3 | No pane ID returned | psmux `split-window` `-P -F` flag handling |
| Test 4 | Text corruption | psmux `send-keys` input encoding / escaping |
| Test 5 | Wrong pane receives text | psmux pane targeting / IPC routing |
| Test 6 | Crash or duplicate IDs | psmux pane ID allocator — needs mutex or atomic counter |
| Test 7 | State lost | psmux session persistence / named pipe lifecycle |
| Test 8 | Wrong format output | psmux format string parser |

The critical-path tests are **1, 2, 3, 4, 5** — if those pass, Claude Code's tmux backend will work through psmux. Tests 6-8 cover robustness under swarm-scale load.
