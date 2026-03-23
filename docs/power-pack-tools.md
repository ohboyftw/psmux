# Windows Terminal Power Pack — Tools Guide

The Power Pack bundles 8 tools that replace slow, verbose Windows defaults with fast, keyboard-driven alternatives. Each tool solves a specific daily pain point.

---

## The Stack

```
┌─────────────────────────────────────────┐
│  fastfetch         System info splash   │  → identity + quick diagnostics
│  starship          Smart shell prompt   │  → git branch, language version, cmd timing
├─────────────────────────────────────────┤
│  psmux             Terminal multiplexer │  → sessions, panes, detach/attach
│    hints mode      Quick-select copy    │  → URLs, paths, hashes → clipboard
│    resurrection    Session persistence  │  → crash recovery
│    layouts         JSON workspaces      │  → repeatable multi-pane setups
├─────────────────────────────────────────┤
│  zoxide            Smart cd             │  → frecency-ranked directory jump
│  fzf               Fuzzy finder         │  → filter anything interactively
│  ripgrep (rg)      Fast search          │  → grep replacement, 10-100x faster
│  fd                Fast find            │  → find replacement, respects .gitignore
│  bat               Smart cat            │  → syntax highlighting, line numbers, git diff
└─────────────────────────────────────────┘
```

---

## Tool-by-Tool Breakdown

### 1. ripgrep (`rg`) — Search code, fast

**What it replaces:** `grep`, `findstr`, `Select-String`

**The pain:** `grep -r "pattern" .` is slow on large codebases. Windows `findstr` has bizarre regex syntax. Neither respects `.gitignore`.

**What it does:** Recursively searches file contents using regex. Automatically skips `.git/`, `node_modules/`, binary files, and anything in `.gitignore`. 10-100x faster than grep on real codebases.

**Daily use:**
```bash
# Find all TODO comments in the project
rg TODO

# Search for a function name, show 3 lines of context
rg -C3 "fn create_window"

# Search only Rust files
rg -t rust "unsafe"

# Search with replacement preview (dry-run refactor)
rg "old_name" --replace "new_name"

# Count matches per file
rg -c "error" src/

# Case-insensitive search
rg -i "config"
```

**Why it matters for AI agent work:** Agent output is dense. When Claude Code writes code across 20 files, `rg "function_name"` finds every reference instantly. No scrolling through `grep` output that includes `node_modules`.

---

### 2. fd — Find files, fast

**What it replaces:** `find`, `dir /s`, `Get-ChildItem -Recurse`

**The pain:** `find . -name "*.rs"` is slow, verbose, and doesn't skip ignored directories. Windows `dir /s` output is unparseable.

**What it does:** Finds files and directories by name pattern. Respects `.gitignore`. Color output. Regex support. 5-10x faster than `find`.

**Daily use:**
```bash
# Find all Rust files
fd -e rs

# Find files matching a pattern
fd "test.*\.py"

# Find and execute a command on each match
fd -e json -x bat {}

# Find only directories
fd -t d src

# Find files modified in the last hour
fd --changed-within 1h

# Find large files (> 1MB)
fd --size +1m
```

**Why it matters:** When you're in a multi-project workspace and need to find `config.toml` — `fd config.toml` returns instantly with the right one, ignoring `target/`, `.git/`, and `node_modules/`.

---

### 3. bat — Read files with syntax highlighting

**What it replaces:** `cat`, `type`, `Get-Content`

**The pain:** `cat file.rs` dumps raw text with no color, no line numbers, no indication of what's changed in git.

**What it does:** Displays file contents with syntax highlighting (300+ languages), line numbers, git diff markers in the gutter, and automatic paging for long files.

**Daily use:**
```bash
# View a file with syntax highlighting
bat src/main.rs

# Show specific line range
bat -r 100:150 src/main.rs

# Show git changes (lines added/modified/deleted)
bat --diff src/main.rs

# Plain output (no decorations) — useful for piping
bat -p file.txt | rg "pattern"

# Use as a man page viewer
export MANPAGER="bat -l man -p"

# Preview files in fzf
fzf --preview 'bat --color=always {}'
```

**Why it matters:** When reviewing AI-generated code, `bat src/hints.rs` shows you syntax-highlighted code with line numbers immediately — no need to open an editor for a quick read.

---

### 4. zoxide (`z`) — Smart directory navigation

**What it replaces:** `cd` with memorized paths

**The pain:** `cd D:\Home\projects\psmux\src\server` every time. Tab completion helps but you still need to know the path.

**What it does:** Learns which directories you visit frequently (frecency = frequency + recency). After a few `cd` commands, `z psmux` takes you directly to `D:\Home\psmux` from anywhere.

**Daily use:**
```bash
# Jump to a frequently visited directory
z psmux         # → D:\Home\psmux
z src           # → D:\Home\psmux\src (most recent 'src' you visited)
z tests         # → D:\Home\psmux\tests

# Interactive picker (with fzf)
zi              # Opens fzf with all ranked directories

# In psmux: Ctrl+b z opens the zoxide picker popup
# Select a directory → new pane opens there
```

**Why it matters:** AI agent workflows involve jumping between project directories constantly — `z canopy`, `z psmux`, `z frontend`. No more typing paths.

---

### 5. fzf — Fuzzy finder for everything

**What it replaces:** Manual scrolling, `Ctrl+R` history search, file pickers

**The pain:** Finding the right file, the right command from history, the right git branch — all require exact memory or slow browsing.

**What it does:** Fuzzy-matches any list interactively. Type a few characters, it narrows the list. Works with any input piped to it.

**Daily use:**
```bash
# Interactive file finder
fzf

# Search command history (Ctrl+R replacement)
history | fzf

# Git branch picker
git branch | fzf | xargs git checkout

# Kill a process interactively
ps aux | fzf | awk '{print $2}' | xargs kill

# Preview files while browsing
fzf --preview 'bat --color=always {}'

# Find and edit a file
nvim $(fzf)

# In psmux: Ctrl+b z uses fzf for the zoxide directory picker
```

**Why it matters:** fzf is the universal "I know it exists but I don't remember the exact name" tool. Combined with ripgrep, bat, and fd, it's a complete search system.

---

### 6. starship — Cross-shell smart prompt

**What it replaces:** Default `PS1` prompt, Oh My Zsh/Posh prompt themes

**The pain:** Default bash prompt shows `user@hostname:path$` — no git info, no language versions, no command timing. Oh My Zsh is Zsh-only. Oh My Posh is PowerShell-first.

**What it does:** Renders a minimal, fast prompt showing:
- Current directory (truncated to 3 levels)
- Git branch + status (dirty/clean/ahead/behind)
- Language version when in a project (Rust, Python, Node)
- Command duration (for commands > 2 seconds)
- Exit status (green `>` on success, red `>` on error)

**What it looks like:**
```
~/Home/psmux  main !3 ?1  rs 1.93.1  2.8s
>
```

**Why it matters:** At a glance you know: which directory, which branch, dirty or clean, which toolchain, and whether the last command was slow. Works in bash, PowerShell, zsh — same prompt everywhere.

---

### 7. fastfetch — System info at a glance

**What it replaces:** `systeminfo`, `winver`, manually checking specs

**The pain:** "What OS version is this?" "How much RAM do I have?" "Which GPU?" — all require different commands on Windows.

**What it does:** Displays a styled system info summary on terminal startup: OS, kernel, CPU, GPU, memory, disk, terminal, shell, uptime, packages.

**What it looks like:**
```
OS: Windows 11 Pro 26200.8037
Host: DESKTOP-ABC1234
Kernel: 10.0.26200
Terminal: Windows Terminal 1.22
Shell: bash 5.2.37
CPU: AMD Ryzen 9 7950X (32) @ 4.50 GHz
GPU: NVIDIA RTX 4090
Memory: 18.2 GiB / 63.7 GiB (29%)
Disk (C:): 234 GiB / 953 GiB (25%)
Uptime: 3 days, 14 hours
```

**Why it matters:** Quick diagnostics when SSHing into machines, filing bug reports, or just showing off your setup. One command, all specs.

---

### 8. psmux — Terminal multiplexer

See [psmux README](../README.md) for full documentation. The multiplexer is the core — all other tools run inside psmux panes.

---

## How They Work Together

**Scenario: Starting a coding session**

```bash
# Terminal opens → fastfetch shows system info
# Starship prompt shows: ~/Home/psmux  main  rs 1.93.1

# Create a multi-pane workspace
psmux new-session -s work

# Ctrl+b z → zoxide picker → select project directory → new pane opens there

# Find a file you half-remember
fd "config" | fzf --preview 'bat --color=always {}'

# Search for a function across the codebase
rg "fn apply_layout" --type rust

# Quick-read a file with highlighting
bat src/layout.rs -r 1500:1560

# Grab a URL from terminal output
# Ctrl+b f → hints overlay → type label → copied to clipboard

# Detach (Ctrl+b d), go home, reattach
psmux attach -t work
```

**Everything is keyboard-driven. No mouse needed.**

---

## Quick Reference Card

| Action | Command | Keybinding |
|--------|---------|-----------|
| Search file contents | `rg "pattern"` | — |
| Find files | `fd "name"` | — |
| View file | `bat file.rs` | — |
| Jump to directory | `z project` | — |
| Fuzzy find anything | `fzf` | Ctrl+R (history) |
| New pane at directory | — | Ctrl+b z |
| Quick-copy URL/path | — | Ctrl+b f |
| Split pane horizontal | — | Ctrl+b % |
| Split pane vertical | — | Ctrl+b " |
| Detach session | — | Ctrl+b d |
| System info | `fastfetch` | (auto on shell start) |
