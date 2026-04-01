# Windows Terminal Power Pack — Product Concept

**Date**: 2026-03-23 (updated 2026-04-01)
**Status**: Feature-complete core / Pre-release polish

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
| **psmux** | tmux-compatible multiplexer (sessions, panes, splits, detach/attach) | Production (92 commands, ~1,100 tests) |
| **Session resurrection** | Crash recovery — auto-saves session state, `apply_snapshot` restores in-server | Done (save/load/apply/CLI) |
| **Hints mode** | Keyboard-driven URL/path/hash quick-copy (like tmux-fingers/vimium) | Done (server + client rendering) |
| **Declarative layouts** | JSON files defining reusable workspace configurations | Done (parser + applier + warm-server path) |
| **Neovim/TUI support** | Mouse (SGR+X10), focus events (?1004h), cursor style (DECSCUSR), bracketed paste | Done |
| **Pane title bars** | Per-pane title display with mode/zoom indicators, status desaturation on unfocus | Done |
| **Agent orchestration** | CustomPaneBackend JSON-RPC, warm pool, wait-pane, remain-on-exit, flicker-free rendering | Production |
| **Zoxide integration** | Frecency-ranked directory picker (`Ctrl+b z`) | Done |
| **Tokyo Night Storm** | Cohesive terminal theme with acrylic transparency | Done |
| **JetBrains Mono NF** | Nerd Font with ligatures and glyphs | Done |

### What Users Get

**For the Neovim Developer on Windows (NEW — 2026-04-01):**
- Mouse clicks, scrolling, drag selection work correctly (SGR + X10 auto-detection)
- Cursor shape follows neovim mode (block→normal, bar→insert) across pane switches
- Focus events trigger `:checktime` / autoread when switching panes
- Bracketed paste prevents cascading autoindent
- Per-pane title bars showing file/buffer context
- **The only Windows multiplexer where neovim works correctly out of the box**

**For the Enterprise Developer:**
- Terminal sessions that survive RDP disconnects and laptop lid-closes
- Detachable sessions (start work at office, continue at home)
- No WSL required — pure Windows, no IT policy violations
- Config files that can be checked into repos and shared across team

**For the AI Agent Developer:**
- CustomPaneBackend JSON-RPC: Claude Code TeammateTool spawns agents in panes programmatically
- `CLAUDE_CODE_NO_FLICKER=1` auto-injected for flicker-free agent rendering
- `remain-on-exit` + `pane_dead` + `respawn-pane` for resilient agent loops
- Declarative layouts: launch 4-pane swarm workspaces from JSON files
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
| ~~Hints mode client rendering~~ | ~~Done~~ — server + client rendering wired | ✅ Complete |
| ~~Resurrection `apply_snapshot`~~ | ~~Done~~ — direct in-server application | ✅ Complete |
| ~~`--layout` warm-server path~~ | ~~Done~~ — `PSMUX_LAYOUT_FILE` env var | ✅ Complete |
| Error messages | Rust debug output | User-friendly error strings for common failures |
| README with screenshots | Current README is dev-focused | Landing page README with GIFs, install instructions |
| Installer script | Manual cargo build + copy | `install.ps1` that installs psmux + zoxide + fzf + font + theme |

### P1 — Should have (first release polish)

| Item | Notes |
|------|-------|
| Tab completion for command prompt | `:` mode Tab → cycle through 92 commands |
| `scoop` package manifest | `scoop install psmux` (Scoop is the Windows dev package manager) |
| Shell integration (prompt marks / OSC 133) | Detect command boundaries for "scroll to previous prompt" — parser support planned |
| `psmux.1` man page equivalent | `psmux --help-all` with full command reference |
| `psmux exec` for programmatic pane commands | Replace fragile send-keys with `docker exec`-style process creation (Tier 0 item #22) |

### P2 — Nice to have (v2)

| Item | Notes |
|------|-------|
| Periodic resurrection saves (30s timer) | Phase 2 of resurrection spec |
| Full tree layout restoration | Currently uses `tiled` approximation |
| Custom hint patterns via config | `set -g hint-patterns "url,path,hash,custom:MY_REGEX"` |
| Plugin system (Lua or WASM) | Extensibility for community contributions |
| Web client | Access sessions from a browser |
| Mycel event bus integration | Push pane lifecycle events to external bus (feature-gated, MVP done) |

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

## Competitive Moat (updated 2026-04-01)

| Competitor | Why psmux wins |
|------------|----------------|
| **WezTerm** (25K stars) | Terminal emulator, not multiplexer. Maintainer rejected AI integration (Discussion #5565). Last release Feb 2024 — stalled. psmux runs INSIDE WezTerm. |
| **Zellij** (30K stars) | Zero Windows support. psmux owns the Windows niche entirely. |
| **Frankenterm** | 482 modules, requires WezTerm GUI, targets 200+ agents. psmux is single binary, headless-capable, targets 5-20 agents. Different weight class. |
| **tmux via WSL** | Filesystem bridge penalty, clipboard friction, mixed PATH, needs IT approval. psmux is pure Windows. |

**Unique position:** Only tool combining Windows-native + tmux compat + neovim support + agent orchestration. No competitor covers even 3 of these 4.

## Positioning

**Not competing with:** WSL, WezTerm (full terminal emulator), Visual Studio integrated terminal

**Competing with:** The absence of tmux on Windows. The gap between "Windows Terminal exists" and "I have a productive terminal workflow."

**Tagline options:**
- "tmux for Windows, built for AI agents"
- "The missing terminal workflow for Windows developers"
- "Linux terminal productivity, native on Windows"
- "The only Windows multiplexer where neovim and agent swarms both work" (NEW)

---

## Success Metrics

| Metric | Target | How to measure |
|--------|--------|---------------|
| GitHub stars | 1000 in 6 months | Already at ~30.3k would be aspirational; 500 realistic |
| Weekly downloads (scoop) | 100/week by month 3 | Scoop download stats |
| Claude Code Windows agent users | Feature in Claude Code docs | Anthropic partnership |
| HN/Reddit front page | 1 post in top 30 | Post "Show HN: tmux for Windows" |
| Enterprise adoption | 3 companies using it | Direct outreach |

---

## Highest-Value Ideas (ranked by impact × feasibility)

*Added 2026-04-01. Ranked by what moves the needle most for adoption.*

### Tier S — Ship-blockers that unlock entire user segments

**1. `install.ps1` one-liner installer**
- Unlocks: Everyone. No installer = no adoption outside Rust developers.
- Effort: Medium (1-2 days). Script exists as concept, needs implementation.
- Impact: 10x. Nobody will `cargo install` a terminal multiplexer. The install experience IS the product for first impressions.

**2. README with GIFs/screenshots**
- Unlocks: Everyone. GitHub README is the landing page.
- Effort: Low (half day). Record 3-4 GIFs: split panes, neovim mouse, agent swarm, session resurrection.
- Impact: 5x. A terminal multiplexer without visual proof is invisible.

### Tier A — High-value features that differentiate

**3. `psmux exec` — programmatic pane command execution**
- Unlocks: AI agent developers. Currently send-keys is the only way to run commands in panes — it's fragile, timing-dependent, and breaks on complex quoting.
- Effort: High (3-5 days). New CtrlReq variant, process creation in pane context, exit code return.
- Impact: 5x for agent adoption. Every agent orchestrator hits the send-keys wall (Canopy went through 5 workaround iterations). This is the #1 pain point in memory.
- Cross-ref: Tier 0 items #22, #24 in improvements list. Subsumes run-shell and send-keys --wait-ready.

**4. OSC 133 prompt marker parsing**
- Unlocks: Power users + agent developers. Enables "scroll to previous prompt" and reliable `#{pane_prompt_ready}` instead of 500ms silence heuristic.
- Effort: Medium (2-3 days). VT parser addition + format variable.
- Impact: 3x. Replaces the biggest remaining heuristic in the codebase.

**5. Scoop package manifest**
- Unlocks: Windows developer community. Scoop is how Windows devs install CLI tools.
- Effort: Low (half day). JSON manifest + bucket repo.
- Impact: 3x for discoverability. `scoop install psmux` is the Windows equivalent of `brew install tmux`.

### Tier B — Polish that compounds over time

**6. Show HN / Reddit launch post**
- Unlocks: Community awareness. Can't get users without telling people it exists.
- Effort: Low (half day writing). Needs README + GIFs first (items 1-2).
- Impact: Variable. One good HN post can drive 500+ stars overnight.
- Prerequisite: Items 1 and 2 must be done first.

**7. `default-shell` option (`set -g default-shell bash`)**
- Unlocks: Agent developers. Currently new-window always opens PowerShell — every agent script must remember `--shell bash`.
- Effort: Low (1 day). Config option + spawn logic.
- Impact: 2x for agent reliability. Eliminates an entire class of silent failures.

**8. Tab completion in `:` command mode**
- Unlocks: Power users. 92 commands are unusable without discoverability.
- Effort: Medium (2 days).
- Impact: 2x for retention. Users who can't find commands leave.

### Tier C — Strategic but longer-term

**9. CustomPaneBackend in Claude Code docs**
- Unlocks: Official Anthropic recognition. psmux as recommended Windows backend.
- Effort: External dependency (Anthropic team).
- Impact: 10x if achieved. Being in official docs is the ultimate distribution.
- Action: Open PR to Claude Code docs once items 1-2 are done.

**10. Control mode server (-C/-CC)**
- Unlocks: Third-party tooling. Any tool that speaks tmux protocol can control psmux.
- Effort: High (already decomposed into 9 sub-tasks in backlog).
- Impact: 3x long-term. Enables ecosystem without building everything ourselves.

### Execution Order

```
Week 1: install.ps1 (#1) + README GIFs (#2) + Scoop manifest (#5)
Week 2: default-shell (#7) + Show HN post (#6)
Week 3: psmux exec (#3)
Week 4: OSC 133 (#4) + tab completion (#8)
Ongoing: Claude Code docs PR (#9), control mode (#10)
```

The first two items (installer + README) gate everything else. No point building features nobody can find or install.
