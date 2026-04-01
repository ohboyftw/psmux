# psmux Demo Recordings

Scripts for recording README GIFs using PowerSession + agg.

## Prerequisites

```powershell
cargo install PowerSession
# agg: gh release download v1.7.0 --repo asciinema/agg --pattern "agg-x86_64-pc-windows-msvc.exe"
```

## Recording

Each demo has a driver script (`demo-*.ps1`) that types commands automatically.
Record by running PowerSession with the script:

```powershell
PowerSession rec -c "pwsh -NoProfile -File demos/demo-01-hero.ps1" demos/hero.cast
agg demos/hero.cast demos/hero.gif --cols 120 --rows 35 --font-size 16
```

Or use the all-in-one recorder:

```powershell
pwsh -NoProfile -File demos/record-all.ps1
```

## Demos

| # | Name | Duration | Shows |
|---|------|----------|-------|
| 01 | Hero | ~12s | New session, split panes, navigate, status bar |
| 02 | Power Pack Tools | ~20s | ripgrep, fd, bat, television, zoxide |
| 03 | Neovim | ~15s | Mouse, cursor shape, focus events, pane switch |
| 04 | Agent Swarm | ~15s | JSON layout, multi-pane agents, capture-pane |
| 05 | Session Lifecycle | ~15s | Detach, reattach, resurrection, resume |
