```
╔═══════════════════════════════════════════════════════════╗
║   ██████╗ ███████╗███╗   ███╗██╗   ██╗██╗  ██╗            ║
║   ██╔══██╗██╔════╝████╗ ████║██║   ██║╚██╗██╔╝            ║
║   ██████╔╝███████╗██╔████╔██║██║   ██║ ╚███╔╝             ║
║   ██╔═══╝ ╚════██║██║╚██╔╝██║██║   ██║ ██╔██╗             ║
║   ██║     ███████║██║ ╚═╝ ██║╚██████╔╝██╔╝ ██╗            ║
║   ╚═╝     ╚══════╝╚═╝     ╚═╝ ╚═════╝ ╚═╝  ╚═╝            ║
║     Born in PowerShell. Made in Rust. 🦀                 ║
║          Terminal Multiplexer for Windows                 ║
╚═══════════════════════════════════════════════════════════╝
```

<p align="center">
  <strong>The native Windows tmux. Born in PowerShell, made in Rust.</strong><br/>
  Full mouse support · tmux themes · tmux config · 92 commands · blazing fast
</p>

<p align="center">
  <a href="#installation">Install</a> ·
  <a href="#usage">Usage</a> ·
  <a href="docs/claude-code.md">Claude Code</a> ·
  <a href="docs/features.md">Features</a> ·
  <a href="docs/compatibility.md">Compatibility</a> ·
  <a href="docs/performance.md">Performance</a> ·
  <a href="docs/plugins.md">Plugins</a> ·
  <a href="docs/keybindings.md">Keys</a> ·
  <a href="docs/scripting.md">Scripting</a> ·
  <a href="docs/configuration.md">Config</a> ·
  <a href="docs/mouse-ssh.md">Mouse/SSH</a> ·
  <a href="docs/faq.md">FAQ</a> ·
  <a href="#related-projects">Related Projects</a>
</p>

---

# psmux

**The real tmux for Windows.** Not a port, not a wrapper, not a workaround.

psmux is a **native Windows terminal multiplexer** built from the ground up in Rust. It uses Windows ConPTY directly, speaks the tmux command language, reads your `.tmux.conf`, and supports tmux themes. All without WSL, Cygwin, or MSYS2.

> 💡 **Tip:** psmux ships with `tmux` and `pmux` aliases. Just type `tmux` and it works!

👀 On Windows 👇

![psmux in action](demo.gif)

## Installation

### Using WinGet

```powershell
winget install psmux
```

### Using Cargo

```powershell
cargo install psmux
```

This installs `psmux`, `pmux`, and `tmux` binaries to your Cargo bin directory.

### Using Scoop

```powershell
scoop bucket add psmux https://github.com/psmux/scoop-psmux
scoop install psmux
```

### Using Chocolatey

```powershell
choco install psmux
```

### From GitHub Releases

Download the latest `.zip` from [GitHub Releases](https://github.com/psmux/psmux/releases) and add to your PATH.

### From Source

```powershell
git clone https://github.com/psmux/psmux.git
cd psmux
cargo build --release
```

Built binaries:

```text
target\release\psmux.exe
target\release\pmux.exe
target\release\tmux.exe
```

### Docker (build environment)

A ready-made Windows container with Rust + MSVC + SSH for building psmux:

```powershell
cd docker
docker build -t psmux-dev .
docker run -d --name psmux-dev -p 127.0.0.1:2222:22 -e ADMIN_PASSWORD=YourPass123! psmux-dev
ssh ContainerAdministrator@localhost -p 2222
```

See [docker/README.md](docker/README.md) for full details.

### Requirements

- Windows 10 or Windows 11
- **PowerShell 7+** (recommended) or cmd.exe
  - Download PowerShell: `winget install --id Microsoft.PowerShell`
  - Or visit: https://aka.ms/powershell

## Why psmux?

If you've used tmux on Linux/macOS and wished you had something like it on Windows, **this is it**. Split panes, multiple windows, session persistence, full mouse support, tmux themes, 92 commands, 126+ format variables, 53 vim copy-mode keys. Your existing `.tmux.conf` works. Full details: **[docs/features.md](docs/features.md)** · **[docs/compatibility.md](docs/compatibility.md)**

## Usage

Use `psmux`, `pmux`, or `tmux` — they're identical:

```powershell
psmux                        # Start a new session
psmux new-session -s work    # Named session
psmux ls                     # List sessions
psmux attach -t work         # Attach to a session
psmux --help                 # Show help
```

## Claude Code Agent Teams

psmux has first-class support for Claude Code agent teams. When Claude Code runs inside a psmux session, teammate agents automatically spawn in separate tmux panes instead of running in-process.

```powershell
psmux new-session -s work    # Start a psmux session
claude                       # Run Claude Code — agent teams just work
```

No extra configuration needed. Full guide: **[docs/claude-code.md](docs/claude-code.md)**

### Claude Code Hooks Integration

Wire psmux into Claude Code's lifecycle with hooks for automatic notifications and output capture.

**Add to `~/.claude/settings.json`:**

```json
{
  "hooks": {
    "Notification": [
      {
        "matcher": "",
        "command": "bash /path/to/psmux/scripts/hooks/on-notification.sh"
      }
    ],
    "Stop": [
      {
        "matcher": "",
        "command": "bash /path/to/psmux/scripts/hooks/on-agent-stop.sh"
      }
    ]
  }
}
```

**What each hook does:**

| Hook | Script | Behavior |
|------|--------|----------|
| `Notification` | `on-notification.sh` | Routes Claude Code notifications to the psmux status bar instead of the system tray |
| `Stop` | `on-agent-stop.sh` | Auto-captures pane scrollback to `~/.psmux/agent-logs/` when an agent finishes |
| (optional) | `on-agent-spawn.sh` | Tags panes with `@agent` metadata on TeammateTool spawn |

Agent logs are saved as `~/.psmux/agent-logs/YYYYMMDD-HHMMSS_<session>_<pane>.log` for post-mortem analysis.

### Power Pack Features (`ohboy-builds`)

| Feature | Keybinding | Description |
|---------|-----------|-------------|
| **Hints mode** | `Ctrl+b f` | Scan pane for URLs, file paths, git hashes — type a label to copy |
| **Session resurrection** | auto | Snapshots saved on structural changes, `psmux resurrect <name>` to restore |
| **Declarative layouts** | `--layout file.json` | Define multi-pane workspaces in JSON, apply with `source-file` |
| **Zoxide picker** | `Ctrl+b z` | Frecency-ranked directory popup via zoxide + fzf |

See [docs/power-pack-tools.md](docs/power-pack-tools.md) for the full tool stack guide (ripgrep, fd, bat, zoxide, fzf, starship, fastfetch).

> **[`ohboy-builds` branch](https://github.com/ohboyftw/psmux/tree/ohboy-builds)** adds production-grade agent orchestration on top of `master`:
>
> **Agent Backend (Protocol v2):**
> - **CustomPaneBackend** — JSON-RPC 2.0 server over Windows named pipes for Claude Code's TeammateTool agent spawning
> - **Spawn with readiness** — `spawn_agent` waits until the pane shell is ready before returning (no more swallowed commands)
> - **Capture with freshness** — `capture` can wait for new output via `data_version` tracking (no more stale reads)
> - **`run_shell`** — Execute commands server-side and get stdout + exit codes directly. Bypasses send-keys entirely
> - **`--shell` flag** — Request `bash`, `pwsh`, or `cmd` per pane on `new-window`/`split-window`/`spawn_agent`. Reported via `#{pane_shell}`
> - **`--bare` aware spawning** — Automatically injects `--bare` for Claude Code agents to skip hooks/LSP overhead
> - **Structured errors** — Machine-readable error codes (`PANE_NOT_FOUND`, `SPAWN_TIMEOUT`, `COMMAND_TIMEOUT`, etc.)
>
> **Remote & Transport:**
> - **Remote tmux control mode** — `attach-remote`, `new-session-remote`, `list-sessions-remote` connect to remote Linux tmux sessions over SSH using `-CC` control mode
> - **DCS passthrough** — `set -g allow-passthrough on` forwards DCS sequences to the host terminal
>
> **Agent Compatibility:**
> - **Claude Code `TeammateTool`** — Auto-detected via `$TMUX`. Tested with 5-8 concurrent agents
> - **Warm pool lifecycle** — Version-stamped warm panes, orphan cleanup on session exit
> - **`send-keys --wait-ready`** — Server-side pane readiness polling before key delivery
> - **`-e KEY=VAL`** — Per-pane environment variables on `new-window`/`split-window`
> - **Launcher script:** [`scripts/Start-ClaudeTeams.ps1`](scripts/Start-ClaudeTeams.ps1) — one-command agent teams setup
>
> **Programmatic Execution (v3.3.0):**
> - **`exec`** — `psmux exec -t %N -- command args...` runs a process in the pane's cwd/env and returns JSON `{"exit_code", "stdout", "stderr"}`. Replaces fragile `send-keys` for programmatic use
> - **`new-window -- command`** — Launch a command directly as the pane's initial process (like tmux). Supports `split-window --` too
> - **`#{pane_exit_code}`** — Format variable exposing the exit code of dead panes. Also available as `#{pane_dead_status}`
> - **`capture-pane --plain`** — Strips all ANSI/VT escape sequences for clean programmatic consumption
> - **`wait-for --file`** — `psmux wait-for --file .done --timeout 60` watches for file creation server-side (250ms polling). Eliminates client-side sentinel polling
> - **`kill-pane` fix** — Immediately removes the window when the last pane is killed (no more dead pane lingering)
> - **Target error handling** — `list-panes -t %nonexistent` and `list-windows -t` return non-zero exit codes
>
> These features are on the [`ohboyftw/psmux`](https://github.com/ohboyftw/psmux/tree/ohboy-builds) fork and not yet merged to upstream `master`.

### Stability & Performance Fixes (`ohboy-builds` v3.3.0)

Critical fixes for production use with Claude Code and agent swarms:

| Fix | Problem | Solution |
|-----|---------|----------|
| **DCS buffer cap** | Malformed DCS sequences grew unboundedly (28GB+ observed) | 10MB hard cap with overflow guard in VT parser |
| **Non-blocking pane writes** | `write_all()` to ConPTY blocked the event loop for 5-10 min when child was busy | AsyncPaneWriter: bounded channel + background drain thread per pane |
| **Server poll debounce** | Continuous PTY output locked server at 1ms polling (58% CPU idle) | 5-tick debounce ramp — responsive during bursts, relaxes when idle |
| **Async snapshot saves** | `save_snapshot()` did synchronous disk I/O on the event loop (50ms antivirus retry) | Background writer thread with 100ms debounce |
| **Passthrough entry limit** | DCS passthrough queue entries had no size limit | 1MB per-entry cap, oversized entries dropped |
| **Env var echo fix** | `SetEnvironment` wrote PowerShell commands to warm pane PTY, echoing visibly | Kill+respawn warm pane with process-level env vars |

These fixes compound: the DCS buffer growth caused memory pressure, which slowed child processes, which filled ConPTY input buffers, which blocked the event loop, which buffered all keybindings for minutes.

## Television Integration

psmux ships with a [cable channel pack](cable/) for [television](https://github.com/alexpasmantier/television) (`tv`) — a fast Rust fuzzy finder. Fuzzy-search your sessions, windows, and panes with live `capture-pane` preview.

```powershell
# Install channels
pwsh cable/install.ps1

# Usage
tv psmux-panes       # Fuzzy pane picker with live content preview
tv psmux-agents      # Browse running agent swarm (@agent, @role metadata)
tv psmux-sessions    # Switch sessions
tv psmux-windows     # Switch windows
tv psmux-keys        # Browse key bindings
```

| Channel | Preview | Actions |
|---------|---------|---------|
| `psmux-panes` | Live `capture-pane` output | Enter=focus, Ctrl+K=kill, Ctrl+C=clipboard |
| `psmux-agents` | Clean agent output | Enter=focus, Ctrl+K=kill, Ctrl+S=send-keys |
| `psmux-sessions` | Pane list per session | Enter=attach, Ctrl+K=kill |
| `psmux-windows` | Pane list per window | Enter=select, Ctrl+K=kill |
| `psmux-keys` | — | Enter=copy command |

## Documentation

| Topic | Description |
|-------|-------------|
| **[Features](docs/features.md)** | Full feature list — mouse, copy mode, layouts, format engine |
| **[Compatibility](docs/compatibility.md)** | tmux command/config compatibility matrix |
| **[Performance](docs/performance.md)** | Benchmarks and optimization details |
| **[Key Bindings](docs/keybindings.md)** | Default keys and customization |
| **[Scripting](docs/scripting.md)** | 92 commands, hooks, targets, pipe-pane |
| **[Configuration](docs/configuration.md)** | Config files, options, environment variables |
| **[Plugins & Themes](docs/plugins.md)** | Plugin ecosystem — Catppuccin, Dracula, Nord, and more |
| **[Mouse Over SSH](docs/mouse-ssh.md)** | SSH mouse support and Windows version requirements |
| **[Claude Code](docs/claude-code.md)** | Agent teams integration guide |
| **[FAQ](docs/faq.md)** | Common questions and answers |

## Related Projects

<table>
  <tr>
    <td align="center" width="50%">
      <a href="https://github.com/psmux/pstop">
        <img src="https://raw.githubusercontent.com/psmux/pstop/master/pstop-demo.gif" width="400" alt="pstop demo" /><br/>
        <b>pstop</b>
      </a><br/>
      <sub>htop for Windows — real-time system monitor with per-core CPU bars, tree view, 7 color schemes</sub><br/>
      <code>cargo install pstop</code>
    </td>
    <td align="center" width="50%">
      <a href="https://github.com/psmux/psnet">
        <img src="https://raw.githubusercontent.com/psmux/psnet/master/image.png" width="400" alt="psnet screenshot" /><br/>
        <b>psnet</b>
      </a><br/>
      <sub>Real-time TUI network monitor — live speed graphs, connections, traffic log, packet sniffer</sub><br/>
      <code>cargo install psnet</code>
    </td>
  </tr>
  <tr>
    <td align="center" width="50%">
      <a href="https://github.com/psmux/Tmux-Plugin-Panel">
        <img src="https://raw.githubusercontent.com/psmux/Tmux-Plugin-Panel/master/screenshot.png" width="400" alt="Tmux Plugin Panel screenshot" /><br/>
        <b>Tmux Plugin Panel</b>
      </a><br/>
      <sub>TUI plugin & theme manager for tmux and psmux — browse, install, update from your terminal</sub><br/>
      <code>cargo install tmuxpanel</code>
    </td>
    <td align="center" width="50%">
      <a href="https://github.com/psmux/omp-manager">
        <img src="https://raw.githubusercontent.com/psmux/omp-manager/master/screenshot.png" width="400" alt="OMP Manager screenshot" /><br/>
        <b>OMP Manager</b>
      </a><br/>
      <sub>Oh My Posh setup wizard — browse 100+ themes, install fonts, configure shells automatically</sub><br/>
      <code>cargo install omp-manager</code>
    </td>
  </tr>
</table>

## License

MIT

## Contributing

Contributions welcome — bug reports, PRs, docs, and test scripts via [GitHub Issues](https://github.com/psmux/psmux/issues).

If psmux helps your Windows workflow, consider giving it a ⭐ on GitHub!

## Star History

[![Star History Chart](https://api.star-history.com/image?repos=psmux/psmux&type=date&legend=top-left)](https://www.star-history.com/?repos=psmux%2Fpsmux&type=date&legend=top-left)

---

<p align="center">
  Made with ❤️ for PowerShell using Rust 🦀
</p>
