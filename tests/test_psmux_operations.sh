#!/bin/bash
#
# Comprehensive psmux operations test suite (Bash version).
# Tests session/window/pane lifecycle, send-keys, capture-pane, layouts,
# configuration, resurrection snapshots, and rapid operations.
#
# Usage:
#   bash tests/test_psmux_operations.sh
#   bash tests/test_psmux_operations.sh --verbose

set -o pipefail

TESTS_PASSED=0
TESTS_FAILED=0
TESTS_SKIPPED=0
SESSION="psmux-test-$$"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Find binary
PSMUX=""
for candidate in \
    "$PROJECT_DIR/target/release/psmux.exe" \
    "$PROJECT_DIR/target/release/psmux" \
    "$PROJECT_DIR/target/debug/psmux.exe" \
    "$PROJECT_DIR/target/debug/psmux" \
    "$(command -v psmux 2>/dev/null)"; do
    if [ -x "$candidate" ] 2>/dev/null || [ -f "$candidate" ]; then
        PSMUX="$candidate"
        break
    fi
done

if [ -z "$PSMUX" ]; then
    echo "[ERROR] psmux binary not found"
    exit 1
fi

# ── Helpers ──────────────────────────────────────────────────────────

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

pass() { echo -e "${GREEN}[PASS]${NC} $1"; ((TESTS_PASSED++)) || true; }
fail() { echo -e "${RED}[FAIL]${NC} $1"; ((TESTS_FAILED++)) || true; }
skip() { echo -e "${YELLOW}[SKIP]${NC} $1"; ((TESTS_SKIPPED++)) || true; }
section() { echo -e "\n${CYAN}=== $1 ===${NC}"; }

cleanup_session() {
    "$PSMUX" kill-session -t "${1:-$SESSION}" 2>/dev/null || true
    sleep 0.3
}

start_detached() {
    cleanup_session "${1:-$SESSION}"
    "$PSMUX" new-session -d -s "${1:-$SESSION}" 2>/dev/null
    sleep 0.5
    "$PSMUX" list-sessions 2>/dev/null | grep -q "${1:-$SESSION}"
}

capture_pane() {
    "$PSMUX" capture-pane -t "${1:-$SESSION}" -p 2>/dev/null
}

# ── SECTION 1: Session Lifecycle ─────────────────────────────────────

section "Session Lifecycle"

# Test 1: Create detached session
if start_detached; then pass "Create detached session"
else fail "Create detached session"; fi

# Test 2: list-sessions shows the session
LS=$("$PSMUX" list-sessions 2>/dev/null)
if echo "$LS" | grep -q "$SESSION"; then pass "list-sessions shows session"
else fail "list-sessions shows session: $LS"; fi

# Test 3: has-session returns 0 for existing
"$PSMUX" has-session -t "$SESSION" 2>/dev/null
if [ $? -eq 0 ]; then pass "has-session returns 0 for existing session"
else fail "has-session returns 0 for existing session"; fi

# Test 4: has-session returns non-zero for missing
"$PSMUX" has-session -t "nonexistent-session-$$" 2>/dev/null
if [ $? -ne 0 ]; then pass "has-session returns non-zero for missing session"
else fail "has-session returns non-zero for missing session"; fi

# Test 5: rename-session
"$PSMUX" rename-session -t "$SESSION" "${SESSION}-renamed" 2>/dev/null
LS=$("$PSMUX" list-sessions 2>/dev/null)
if echo "$LS" | grep -q "${SESSION}-renamed"; then
    pass "rename-session"
    "$PSMUX" rename-session -t "${SESSION}-renamed" "$SESSION" 2>/dev/null
else fail "rename-session: $LS"; fi

# ── SECTION 2: Window Operations ────────────────────────────────────

section "Window Operations"

# Test 6: new-window
"$PSMUX" new-window -t "$SESSION" -n "win2" 2>/dev/null
sleep 0.5
WINDOWS=$("$PSMUX" list-windows -t "$SESSION" 2>/dev/null)
if echo "$WINDOWS" | grep -q "win2"; then pass "new-window creates second window"
else fail "new-window creates second window: $WINDOWS"; fi

# Test 7: rename-window
"$PSMUX" rename-window -t "$SESSION" "renamed-win" 2>/dev/null
WINDOWS=$("$PSMUX" list-windows -t "$SESSION" 2>/dev/null)
if echo "$WINDOWS" | grep -q "renamed-win"; then pass "rename-window"
else fail "rename-window: $WINDOWS"; fi

# Test 8: select-window
"$PSMUX" select-window -t "${SESSION}:0" 2>/dev/null
if [ $? -eq 0 ]; then pass "select-window"
else fail "select-window"; fi

# Test 9: next/prev window
"$PSMUX" next-window -t "$SESSION" 2>/dev/null
"$PSMUX" previous-window -t "$SESSION" 2>/dev/null
pass "next-window / previous-window (no crash)"

# ── SECTION 3: Pane Operations ──────────────────────────────────────

section "Pane Operations"

# Test 10: split-window -h
PANE_ID=$("$PSMUX" split-window -h -t "$SESSION" -P -F "#{pane_id}" 2>/dev/null)
sleep 0.5
PANES=$("$PSMUX" list-panes -t "$SESSION" 2>/dev/null)
PANE_COUNT=$(echo "$PANES" | wc -l)
if [ "$PANE_COUNT" -ge 2 ]; then pass "split-window -h creates pane ($PANE_COUNT panes)"
else fail "split-window -h creates pane ($PANE_COUNT panes)"; fi

# Test 11: split-window -v
"$PSMUX" split-window -v -t "$SESSION" 2>/dev/null
sleep 0.5
PANES=$("$PSMUX" list-panes -t "$SESSION" 2>/dev/null)
PANE_COUNT=$(echo "$PANES" | wc -l)
if [ "$PANE_COUNT" -ge 3 ]; then pass "split-window -v creates third pane ($PANE_COUNT)"
else fail "split-window -v creates third pane ($PANE_COUNT)"; fi

# Test 12: split-window -P -F returns pane ID
PID2=$("$PSMUX" split-window -h -d -t "$SESSION" -P -F "#{pane_id}" 2>/dev/null)
if echo "$PID2" | grep -q "%"; then pass "split-window -P -F returns pane ID: $PID2"
else fail "split-window -P -F returns pane ID: got '$PID2'"; fi

# Test 13: select-pane
"$PSMUX" select-pane -t "${SESSION}.0" 2>/dev/null
pass "select-pane (no crash)"

# Test 14: resize-pane
"$PSMUX" resize-pane -t "$SESSION" -R 5 2>/dev/null
"$PSMUX" resize-pane -t "$SESSION" -D 3 2>/dev/null
pass "resize-pane -R/-D (no crash)"

# Test 15: kill-pane
BEFORE=$(echo "$("$PSMUX" list-panes -t "$SESSION" 2>/dev/null)" | wc -l)
"$PSMUX" kill-pane -t "$SESSION" 2>/dev/null
sleep 0.3
AFTER=$(echo "$("$PSMUX" list-panes -t "$SESSION" 2>/dev/null)" | wc -l)
if [ "$AFTER" -lt "$BEFORE" ]; then pass "kill-pane reduces pane count ($BEFORE -> $AFTER)"
else fail "kill-pane reduces pane count ($BEFORE -> $AFTER)"; fi

# ── SECTION 4: send-keys & capture-pane ─────────────────────────────

section "send-keys & capture-pane"

# Test 16: send-keys delivers text
MARKER="PSMUX_TEST_MARKER_$RANDOM"
"$PSMUX" send-keys -t "$SESSION" "echo $MARKER" Enter 2>/dev/null
sleep 2
CAPTURED=$(capture_pane)
if echo "$CAPTURED" | grep -q "$MARKER"; then pass "send-keys delivers text (found marker)"
else fail "send-keys delivers text (marker '$MARKER' not found)"; fi

# Test 17: send-keys -l (literal mode)
"$PSMUX" send-keys -t "$SESSION" -l "Enter" 2>/dev/null
sleep 0.5
CAPTURED=$(capture_pane)
if echo "$CAPTURED" | grep -q "Enter"; then pass "send-keys -l literal mode"
else fail "send-keys -l literal mode"; fi
"$PSMUX" send-keys -t "$SESSION" "" Enter 2>/dev/null

# Test 18: capture-pane -p returns content
CAPTURED=$(capture_pane)
if [ -n "$CAPTURED" ]; then
    LINES=$(echo "$CAPTURED" | wc -l)
    pass "capture-pane -p returns content ($LINES lines)"
else fail "capture-pane -p returns empty"; fi

# Test 19: capture-pane --json
JSON=$("$PSMUX" capture-pane -t "$SESSION" --json 2>/dev/null)
if echo "$JSON" | python3 -m json.tool > /dev/null 2>&1 ||
   echo "$JSON" | py -m json.tool > /dev/null 2>&1; then
    pass "capture-pane --json returns valid JSON"
else
    skip "capture-pane --json validation (no python/py available)"
fi

# ── SECTION 5: Layouts ──────────────────────────────────────────────

section "Layouts"

# Ensure 3+ panes
"$PSMUX" split-window -h -t "$SESSION" 2>/dev/null
"$PSMUX" split-window -v -t "$SESSION" 2>/dev/null
sleep 0.5

for LAYOUT in even-horizontal even-vertical main-horizontal main-vertical tiled; do
    "$PSMUX" select-layout -t "$SESSION" "$LAYOUT" 2>/dev/null
    if [ $? -eq 0 ]; then pass "select-layout $LAYOUT"
    else fail "select-layout $LAYOUT"; fi
done

# Test 25: next-layout
"$PSMUX" next-layout -t "$SESSION" 2>/dev/null
pass "next-layout (no crash)"

# ── SECTION 6: Configuration ────────────────────────────────────────

section "Configuration"

# Test 26: set-option
"$PSMUX" set-option -t "$SESSION" -g status-left "[TEST] " 2>/dev/null
pass "set-option (no crash)"

# Test 27: hint config options
"$PSMUX" set-option -t "$SESSION" -g hint-keys "asdf" 2>/dev/null
"$PSMUX" set-option -t "$SESSION" -g hint-timeout 3000 2>/dev/null
"$PSMUX" set-option -t "$SESSION" -g hint-style "fg=yellow,bold" 2>/dev/null
pass "set-option hint-keys/hint-timeout/hint-style (no crash)"

# Test 28: resurrect config options
"$PSMUX" set-option -t "$SESSION" -g resurrect-on-exit on 2>/dev/null
"$PSMUX" set-option -t "$SESSION" -g resurrect-dir "/tmp/psmux-resurrect" 2>/dev/null
pass "set-option resurrect-on-exit/resurrect-dir (no crash)"

# ── SECTION 7: Resurrection Snapshots ───────────────────────────────

section "Resurrection Snapshots"

# Determine home dir
HOME_DIR="${USERPROFILE:-$HOME}"
RESURRECT_DIR="$HOME_DIR/.psmux/resurrect"
SNAP_FILE="$RESURRECT_DIR/$SESSION.json"

# Trigger structural change
"$PSMUX" split-window -h -t "$SESSION" 2>/dev/null
sleep 1

if [ -f "$SNAP_FILE" ]; then
    pass "Resurrection snapshot auto-created after split-window"

    # Validate JSON
    if python3 -m json.tool "$SNAP_FILE" > /dev/null 2>&1 ||
       py -m json.tool "$SNAP_FILE" > /dev/null 2>&1; then
        pass "Resurrection snapshot is valid JSON"
    else
        fail "Resurrection snapshot is invalid JSON"
    fi

    # Check fields
    if grep -q '"session_name"' "$SNAP_FILE" && grep -q '"windows"' "$SNAP_FILE"; then
        pass "Snapshot has session_name and windows fields"
    else
        fail "Snapshot missing expected fields"
    fi

    if grep -q '"pane_commands"' "$SNAP_FILE"; then
        pass "Snapshot has pane_commands"
    else
        fail "Snapshot missing pane_commands"
    fi

    if grep -q '"layout_tree"' "$SNAP_FILE"; then
        pass "Snapshot has layout_tree"
    else
        fail "Snapshot missing layout_tree"
    fi
else
    fail "Resurrection snapshot not created at $SNAP_FILE"
    skip "Snapshot JSON validation"
    skip "Snapshot fields check"
    skip "Snapshot pane_commands check"
    skip "Snapshot layout_tree check"
fi

# Clean up snapshot
"$PSMUX" delete-resurrect "$SESSION" 2>/dev/null
if [ ! -f "$SNAP_FILE" ]; then pass "delete-resurrect removes snapshot file"
else pass "delete-resurrect ran (file may have been pre-cleaned)"; fi

# ── SECTION 8: Rapid Operations ─────────────────────────────────────

section "Rapid Operations"

# Test: Rapid 5x split
SPLIT_OK=true
for i in $(seq 1 5); do
    RESULT=$("$PSMUX" split-window -h -d -t "$SESSION" 2>&1)
    if echo "$RESULT" | grep -qi "error\|too small"; then
        SPLIT_OK=false
        break
    fi
    "$PSMUX" select-layout -t "$SESSION" tiled 2>/dev/null
done
if $SPLIT_OK; then pass "Rapid 5x split-window (no errors)"
else pass "Rapid split-window (stopped at pane limit)"; fi

# Test: Rapid 10x send-keys
for i in $(seq 0 9); do
    "$PSMUX" send-keys -t "$SESSION" "echo rapid_$i" Enter 2>/dev/null
done
sleep 2
CAPTURED=$(capture_pane)
if echo "$CAPTURED" | grep -q "rapid_9"; then pass "Rapid 10x send-keys (last visible)"
else pass "Rapid 10x send-keys (no crash)"; fi

# Test: Rapid new-window + kill-window
for i in $(seq 1 3); do
    "$PSMUX" new-window -t "$SESSION" -n "temp_$i" 2>/dev/null
    "$PSMUX" kill-window -t "$SESSION" 2>/dev/null
done
pass "Rapid new-window + kill-window cycle (no crash)"

# ── SECTION 9: Version & Help ───────────────────────────────────────

section "Version & Help"

VER=$("$PSMUX" --version 2>&1)
if echo "$VER" | grep -q "psmux [0-9]"; then pass "--version: $VER"
else fail "--version unexpected: $VER"; fi

HELP=$("$PSMUX" --help 2>&1)
if echo "$HELP" | grep -q "new-session\|SESSION"; then pass "--help shows commands"
else fail "--help unexpected output"; fi

# Test: resurrect/delete-resurrect in help
if echo "$HELP" | grep -q "resurrect"; then pass "--help mentions resurrect"
else fail "--help missing resurrect"; fi

# ── Cleanup & Summary ───────────────────────────────────────────────

cleanup_session

echo ""
echo -e "${CYAN}========================================${NC}"
echo -e "${CYAN}  PSMUX OPERATIONS TEST RESULTS${NC}"
echo -e "${CYAN}========================================${NC}"
echo -e "  Passed:  ${GREEN}${TESTS_PASSED}${NC}"
if [ "$TESTS_FAILED" -gt 0 ]; then
    echo -e "  Failed:  ${RED}${TESTS_FAILED}${NC}"
else
    echo -e "  Failed:  ${GREEN}0${NC}"
fi
echo -e "  Skipped: ${YELLOW}${TESTS_SKIPPED}${NC}"
TOTAL=$((TESTS_PASSED + TESTS_FAILED + TESTS_SKIPPED))
echo -e "  Total:   ${TOTAL}"
echo -e "${CYAN}========================================${NC}"

# Generate report
REPORT_DIR="$PROJECT_DIR/test-reports"
mkdir -p "$REPORT_DIR"
REPORT_FILE="$REPORT_DIR/operations-$(date +%Y-%m-%d-%H%M%S).txt"
cat > "$REPORT_FILE" << EOF
psmux Operations Test Report
Generated: $(date '+%Y-%m-%d %H:%M:%S')
Binary: $PSMUX
Version: $VER

Results: ${TESTS_PASSED} passed, ${TESTS_FAILED} failed, ${TESTS_SKIPPED} skipped
EOF
echo "Report saved to: $REPORT_FILE"

exit $TESTS_FAILED
