# Boundary contract tests for hjkl + g/G picker navigation (issue #259 port)
#
# Source-grep based — no live process needed. These tests pin the input-handler
# match arms in src/client.rs so a future refactor cannot silently regress
# tmux mode-tree navigation parity for choose-session, choose-tree, and
# list-keys (the three pickers ohboy carries).
#
# Counterparts in upstream tests/test_issue259_picker_hjkl.ps1 also include
# buffer_chooser and srv_customize handler checks — those are intentionally
# omitted here because ohboy-builds does not yet carry buffer_chooser or the
# customize-mode overlay (deferred per .claude/internal/ohboy-builds-backlog.md
# line 115).

$ErrorActionPreference = "Stop"
$Source = Join-Path $PSScriptRoot "..\src\client.rs"

if (-not (Test-Path $Source)) {
    throw "Source not found: $Source"
}
$src = Get-Content $Source -Raw

$failed = 0
$passed = 0

function Assert-Match {
    param([string]$Pattern, [string]$Description)
    if ($script:src -match $Pattern) {
        Write-Host "  PASS: $Description" -ForegroundColor Green
        $script:passed++
    } else {
        Write-Host "  FAIL: $Description" -ForegroundColor Red
        Write-Host "        pattern: $Pattern"
        $script:failed++
    }
}

Write-Host "`n=== Contract 1: session_chooser hjkl + g/G handlers ===" -ForegroundColor Cyan
Assert-Match "KeyCode::Char\('k'\) if session_chooser" "session_chooser handles Char('k') -> up"
Assert-Match "KeyCode::Char\('j'\) if session_chooser" "session_chooser handles Char('j') -> down"
Assert-Match "KeyCode::Char\('h'\) if session_chooser" "session_chooser handles Char('h') -> up"
Assert-Match "KeyCode::Char\('l'\) if session_chooser" "session_chooser handles Char('l') -> down"
Assert-Match "KeyCode::Char\('g'\) if session_chooser" "session_chooser handles Char('g') -> top"
Assert-Match "KeyCode::Char\('G'\) if session_chooser => \{ session_selected = session_entries\.len\(\)\.saturating_sub\(1\)" "session_chooser handles Char('G') -> bottom (saturating_sub guards empty list)"

Write-Host "`n=== Contract 2: tree_chooser hjkl + g/G handlers ===" -ForegroundColor Cyan
Assert-Match "KeyCode::Char\('k'\) if tree_chooser" "tree_chooser handles Char('k') -> up"
Assert-Match "KeyCode::Char\('j'\) if tree_chooser" "tree_chooser handles Char('j') -> down"
Assert-Match "KeyCode::Char\('h'\) if tree_chooser" "tree_chooser handles Char('h') -> up"
Assert-Match "KeyCode::Char\('l'\) if tree_chooser" "tree_chooser handles Char('l') -> down"
Assert-Match "KeyCode::Char\('g'\) if tree_chooser => \{ tree_selected = 0" "tree_chooser handles Char('g') -> top"
Assert-Match "KeyCode::Char\('G'\) if tree_chooser => \{ tree_selected = tree_entries\.len\(\)\.saturating_sub\(1\)" "tree_chooser handles Char('G') -> bottom"

Write-Host "`n=== Contract 3: keys_viewer h/l/g/G additions (j/k pre-existing) ===" -ForegroundColor Cyan
Assert-Match "KeyCode::Char\('h'\) if keys_viewer" "keys_viewer handles Char('h') -> scroll up"
Assert-Match "KeyCode::Char\('l'\) if keys_viewer" "keys_viewer handles Char('l') -> scroll down"
Assert-Match "KeyCode::Char\('g'\) if keys_viewer => \{ keys_viewer_scroll = 0" "keys_viewer handles Char('g') -> top"
Assert-Match "KeyCode::Char\('G'\) if keys_viewer => \{ keys_viewer_scroll = keys_viewer_lines\.len\(\)\.saturating_sub\(1\)" "keys_viewer handles Char('G') -> bottom"
Assert-Match "KeyCode::Char\('j'\) if keys_viewer" "keys_viewer still handles Char('j') (pre-existing)"
Assert-Match "KeyCode::Char\('k'\) if keys_viewer" "keys_viewer still handles Char('k') (pre-existing)"

Write-Host "`n=== Regression 4: arrow keys still work in session_chooser/tree_chooser ===" -ForegroundColor Cyan
Assert-Match "KeyCode::Up if session_chooser" "Up arrow still bound in session_chooser"
Assert-Match "KeyCode::Down if session_chooser" "Down arrow still bound in session_chooser"
Assert-Match "KeyCode::Up if tree_chooser" "Up arrow still bound in tree_chooser"
Assert-Match "KeyCode::Down if tree_chooser" "Down arrow still bound in tree_chooser"
Assert-Match "KeyCode::Enter if session_chooser" "Enter still bound in session_chooser (entry switch)"
Assert-Match "KeyCode::Esc if session_chooser" "Esc still bound in session_chooser (close)"
Assert-Match "KeyCode::Char\('x'\) if session_chooser" "x still bound in session_chooser (kill — ohboy extension)"

Write-Host "`n=== Regression 5: keys_viewer arrow + page navigation preserved ===" -ForegroundColor Cyan
Assert-Match "KeyCode::Up if keys_viewer" "Up arrow still bound in keys_viewer"
Assert-Match "KeyCode::Down if keys_viewer" "Down arrow still bound in keys_viewer"
Assert-Match "KeyCode::PageUp if keys_viewer" "PageUp still bound in keys_viewer"
Assert-Match "KeyCode::PageDown if keys_viewer" "PageDown still bound in keys_viewer"
Assert-Match "KeyCode::Home if keys_viewer => \{ keys_viewer_scroll = 0" "Home still bound in keys_viewer"
Assert-Match "KeyCode::End if keys_viewer" "End still bound in keys_viewer"

Write-Host "`n=== Regression 6: scroll-indicator overlay (Top/Bot/%) preserved ===" -ForegroundColor Cyan
# The render code path must still emit Top/Bot/% indicator. This is an
# ohboy-only feature absent upstream; hjkl handlers must NOT have disturbed it.
Assert-Match '"Top"\.to_string\(\)' "keys_viewer render: 'Top' indicator emitted"
Assert-Match '"Bot"\.to_string\(\)' "keys_viewer render: 'Bot' indicator emitted"
Assert-Match 'format!\("\{\}%", pct\)' "keys_viewer render: percentage indicator emitted"
Assert-Match "if keys_viewer_lines\.len\(\) > visible_h" "keys_viewer render: indicator gated on overflow"
# tree_chooser dynamic-overlay scroll tracking (also ohboy-only) must survive
Assert-Match "if tree_selected >= tree_scroll \+ visible_h" "tree_chooser scroll tracking preserved"
Assert-Match "if tree_selected < tree_scroll" "tree_chooser scroll-up tracking preserved"

Write-Host "`n=== Summary ===" -ForegroundColor Cyan
Write-Host "Passed: $passed" -ForegroundColor Green
Write-Host "Failed: $failed" -ForegroundColor $(if ($failed -gt 0) { "Red" } else { "Green" })

if ($failed -gt 0) { exit 1 } else { exit 0 }
