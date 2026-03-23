# Windows Terminal Power Pack — Product Concept

**Date**: 2026-03-23
**Status**: Concept / Pre-brainstorming

---

## Who MUST Use Windows for Development?

### The Niche Users

These aren't people who choose Windows — they're people who **can't leave it**.

**1. Enterprise/Corporate Developers (largest segment)**
- Fortune 500 IT policies mandate Windows + Active Directory
- Healthcare, finance, defense, government — regulated industries with locked-down SOEs (Standard Operating Environments)
- Can't install WSL without IT approval (Group Policy blocks it)
- Can't dual-boot (BitLocker + Secure Boot + corporate imaging)
- Often stuck with Windows 10 LTSC (no Store, limited updates)
- **Size**: ~15-20M developers globally work under Windows-only enterprise policies

**2. .NET / Visual Studio Ecosystem Developers**
- C#, F#, WPF, WinForms, MAUI, Blazor, Unity game developers
- Visual Studio is Windows-first (VS for Mac was discontinued 2024)
- DirectX, Win32 API, COM interop — code that literally can't run elsewhere
- Windows driver developers (WDK)
- **Size**: ~6-8M active .NET developers

**3. Game Developers**
- Unreal Engine, Unity, Godot — Windows is the primary build target
- DirectX 12, Vulkan on Windows, GPU debugging tools (PIX, NSight)
- Anti-cheat SDKs (EasyAntiCheat, BattlEye) — Windows-only
- Console dev kits (Xbox GDK) require Windows
- **Size**: ~2-3M game developers

**4. Hardware / Embedded / IoT Developers**
- CAD tools (SolidWorks, Altium, KiCad) are Windows-primary
- Firmware flashing tools (J-Link, ST-Link) have best Windows support
- PLC programming (Siemens TIA Portal, Allen-Bradley) — Windows-only
- FPGA tools (Vivado, Quartus) — Windows-primary
- **Size**: ~3-5M embedded/hardware engineers

**5. Data Science / ML Engineers at Windows Shops**
- Corporate Jupyter environments on Windows
- GPU workstations with CUDA — Windows is the default OS for NVIDIA workstation GPUs
- Power BI, Azure ML Studio integrations
- Can't SSH into Linux boxes (firewall/VPN restrictions)
- **Size**: ~2-3M

**6. AI Agent Developers on Windows (emerging, our primary target)**
- Using Claude Code, Cursor, Copilot Workspace, Aider, etc.
- Need tmux-like session management for multi-agent orchestration
- Can't use tmux natively (no Linux)
- WSL adds friction: filesystem bridge penalty, clipboard issues, mixed PATH
- Want to run agent swarms with real PTY panes, not background processes
- **Size**: ~500K-1M and growing fast (this is where the market is moving)

---

## The Pain (Why These Users Suffer)

**macOS/Linux developers get for free:**
- tmux (sessions, panes, detach/attach, scripting)
- zsh + oh-my-zsh (autocomplete, themes, plugins)
- Native package managers (brew, apt)
- SSH multiplexing with control sockets
- Clipboard integration that just works
- fzf, ripgrep, fd, bat, exa — all assume Unix

**Windows developers get:**
- Windows Terminal (good renderer, no multiplexer)
- PowerShell (powerful but verbose, no tmux equivalent)
- ConPTY (good but quirky — mouse injection, VTI mode issues)
- WSL (works but adds a translation layer, dual-OS maintenance, filesystem perf hit)
- No native session persistence across restarts
- No keyboard-driven workflow comparable to tmux + vim

**The gap:** Windows Terminal is a great *terminal emulator* but a terrible *terminal workflow*. There's no cohesive "power user" experience that matches what tmux + zsh + fzf gives Linux/Mac users.

---

## What the Power Pack Delivers

### Value Proposition

**"The Linux terminal power-user experience, native on Windows, purpose-built for AI-assisted development."**

One install. One config. Everything works together.

### The Stack

| Component | What it does | Status |
|-----------|-------------|--------|
| **psmux** | tmux-compatible multiplexer (sessions, panes, splits, detach/attach) | Production (92 commands, 924 tests) |
| **Session resurrection** | Crash recovery — auto-saves session state, restore on reattach | MVP done (save/load/CLI) |
| **Hints mode** | Keyboard-driven URL/path/hash quick-copy (like tmux-fingers/vimium) | MVP done (scanning + labels + server integration) |
| **Declarative layouts** | JSON files defining reusable workspace configurations | MVP done (parser + applier) |
| **Zoxide integration** | Frecency-ranked directory picker (`Ctrl+b z`) | Done |
| **Tokyo Night Storm** | Cohesive terminal theme with acrylic transparency | Done |
| **JetBrains Mono NF** | Nerd Font with ligatures and glyphs | Done |
| **Agent orchestration** | Claude Code TeammateTool backend, swarm spawning, `send-keys`/`capture-pane` | Production |

### What Users Get

**For the Enterprise Developer:**
- Terminal sessions that survive RDP disconnects and laptop lid-closes
- Detachable sessions (start work at office, continue at home)
- No WSL required — pure Windows, no IT policy violations
- Config files that can be checked into repos and shared across team

**For the AI Agent Developer:**
- Multi-agent workspaces defined in JSON (launch 4-pane swarm with one command)
- `send-keys` / `capture-pane` API for programmatic agent control
- Session resurrection after crashes (AI sessions are long-running)
- Hints mode for grabbing commit hashes, URLs, file paths from agent output

**For the Power User Who Misses Linux:**
- tmux keybindings they already know
- zoxide smart-cd (`z project` instead of `cd D:\Home\project`)
- fzf everywhere (directory picker, file finder)
- Copy mode with vim keybindings
- A terminal that looks like it belongs on r/unixporn, not r/windows

---

## What's Needed to Ship

### P0 — Must have (blocks release)

| Item | Current State | Work Remaining |
|------|--------------|----------------|
| Hints mode client rendering | Server serializes, client doesn't paint | Implement dimming + label overlay in `client.rs` |
| Resurrection `apply_snapshot` | Works via subprocess workaround | Rewrite as direct in-server application |
| `--layout` warm-server path | Only works for cold start (env var) | Wire through warm-server claiming flow |
| Error messages | Rust debug output | User-friendly error strings for common failures |
| README with screenshots | Current README is dev-focused | Landing page README with GIFs, install instructions |
| Installer script | Manual cargo build + copy | `install.ps1` that installs psmux + zoxide + fzf + font + theme |

### P1 — Should have (first release polish)

| Item | Notes |
|------|-------|
| Tab completion for command prompt | `:` mode Tab → cycle through 92 commands |
| `scoop` package manifest | `scoop install psmux` (Scoop is the Windows dev package manager) |
| Shell integration (prompt marks) | Detect command boundaries for "scroll to previous prompt" |
| `psmux.1` man page equivalent | `psmux --help-all` with full command reference |

### P2 — Nice to have (v2)

| Item | Notes |
|------|-------|
| Periodic resurrection saves (30s timer) | Phase 2 of resurrection spec |
| Full tree layout restoration | Currently uses `tiled` approximation |
| Custom hint patterns via config | `set -g hint-patterns "url,path,hash,custom:MY_REGEX"` |
| Plugin system (Lua or WASM) | Extensibility for community contributions |
| Web client | Access sessions from a browser |

---

## Distribution Strategy

### Primary: Scoop (Windows package manager for developers)

```powershell
scoop bucket add ohboy https://github.com/ohboyftw/scoop-bucket
scoop install psmux
```

Scoop handles: binary download, PATH setup, shims. No admin required.

### Secondary: Cargo (Rust developers)

```bash
cargo install psmux
```

Already works. Doesn't install dependencies (zoxide, fzf, font).

### Tertiary: GitHub Releases + install.ps1

```powershell
irm https://raw.githubusercontent.com/ohboyftw/psmux/master/install.ps1 | iex
```

One-line install that:
1. Downloads psmux binary
2. Installs zoxide + fzf via winget
3. Installs JetBrains Mono NF
4. Applies Tokyo Night Storm theme to Windows Terminal
5. Creates `~/.psmux.conf` with zoxide keybinding
6. Adds zoxide init to shell profile

### Existing: Chocolatey

Already packaged in `packages/chocolatey/`. Needs update for new features.

---

## Positioning

**Not competing with:** WSL, WezTerm (full terminal emulator), Visual Studio integrated terminal

**Competing with:** The absence of tmux on Windows. The gap between "Windows Terminal exists" and "I have a productive terminal workflow."

**Tagline options:**
- "tmux for Windows, built for AI agents"
- "The missing terminal workflow for Windows developers"
- "Linux terminal productivity, native on Windows"

---

## Success Metrics

| Metric | Target | How to measure |
|--------|--------|---------------|
| GitHub stars | 1000 in 6 months | Already at ~30.3k would be aspirational; 500 realistic |
| Weekly downloads (scoop) | 100/week by month 3 | Scoop download stats |
| Claude Code Windows agent users | Feature in Claude Code docs | Anthropic partnership |
| HN/Reddit front page | 1 post in top 30 | Post "Show HN: tmux for Windows" |
| Enterprise adoption | 3 companies using it | Direct outreach |
