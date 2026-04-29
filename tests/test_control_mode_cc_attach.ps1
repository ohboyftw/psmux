# TODO Stage 1D — `-CC` iTerm2-shape attach.
#
# Pre-state: scaffolding from Stage 1A only.
# Stage 1B: dispatch_control_command lands.
# Stage 1C: stdio<->pipe relay in main.rs lands.
# Stage 1D: this test gets a real implementation that runs `psmux -CC attach`,
# asserts the DCS opener `\x1bP1000p` is emitted, that initial state burst
# arrives, and that subsequent splits produce `%window-add` notifications.
#
# Reference matrix row: C2 in design-control-mode-vs-custompanebackend.md §7.

Write-Host "test_control_mode_cc_attach.ps1: STUB — Stage 1D will fill in"
exit 0
