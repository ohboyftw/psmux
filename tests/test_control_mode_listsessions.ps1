# TODO Stage 1D — `list-sessions -F` over the control socket.
#
# Pre-state: scaffolding from Stage 1A only.
# Stage 1B: dispatch_control_command + format_list_sessions land.
# Stage 1D: this test gets a real implementation that issues
# `list-sessions -F "#{session_name}: #{session_windows}"` over a `-CC`
# attach, asserts the response uses the requested format, asserts warm
# panes are excluded from session_windows count.
#
# Reference matrix row: C10 in design-control-mode-vs-custompanebackend.md §7.

Write-Host "test_control_mode_listsessions.ps1: STUB — Stage 1D will fill in"
exit 0
