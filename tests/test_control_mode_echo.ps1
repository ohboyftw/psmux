# TODO Stage 1D — `-C` echo round-trip.
#
# Pre-state: scaffolding from Stage 1A only.
# Stage 1B: dispatch_control_command lands.
# Stage 1C: stdio<->pipe relay in main.rs lands.
# Stage 1D: this test gets a real implementation that runs `psmux -C attach`,
# writes a `list-sessions` line, asserts the echoed command + response come
# back without DCS framing.
#
# Reference matrix row: C1 in design-control-mode-vs-custompanebackend.md §7.

Write-Host "test_control_mode_echo.ps1: STUB — Stage 1D will fill in"
exit 0
