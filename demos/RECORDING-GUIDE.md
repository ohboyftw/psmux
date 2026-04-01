# Recording psmux Demos — Step-by-Step Guide

## Prerequisites

### Step 1: Install PowerSession (terminal recorder)

```powershell
cargo install PowerSession
```

Verify: `PowerSession --version` → `PowerSession 0.1.15`

### Step 2: Install agg (GIF renderer)

```powershell
gh release download v1.7.0 --repo asciinema/agg --pattern "agg-x86_64-pc-windows-msvc.exe" --dir "$env:LOCALAPPDATA\psmux"
Rename-Item "$env:LOCALAPPDATA\psmux\agg-x86_64-pc-windows-msvc.exe" "agg.exe"
```

Verify: `agg --version` → `agg 1.7.0`

### Step 3: Install ffmpeg (MP4 + subtitles, optional)

```powershell
winget install Gyan.FFmpeg
```

Verify: `ffmpeg -version`

### Step 4: Build psmux from ohboy-builds

```powershell
cd D:\Home\psmux
cargo build --release
```

Verify: `psmux --version`

---

## Recording a Single Demo

### Step 5: Pick a demo to record

| Demo | Script | Output | Duration |
|------|--------|--------|----------|
| 01 Hero | `demo-01-hero.ps1` | Splits, navigate, zoom, rename | ~30s |
| 02 Power Pack | `demo-02-powerpack-tools.ps1` | rg, fd, bat, tv, zoxide, fzf | ~45s |
| 03 Neovim | `demo-03-neovim.ps1` | Cursor shapes, focus events, pane switch | ~35s |
| 04 Agent Swarm | `demo-04-agent-swarm.ps1` | Multi-pane agents, send-keys, capture | ~40s |
| 05 Session Lifecycle | `demo-05-session-lifecycle.ps1` | Detach, reattach, resurrection | ~40s |

### Step 6: Set your terminal to a clean state

- Resize Windows Terminal to at least **120 columns x 35 rows**
- Use a dark theme (Tokyo Night Storm recommended)
- Close other psmux sessions: `psmux kill-server 2>$null`
- Clear the screen: `cls`

### Step 7: Start the recording

```powershell
PowerSession rec -c "pwsh -NoProfile -File demos/demo-01-hero.ps1" demos/hero.cast
```

**What happens:**
1. The script creates a psmux session and drives it via `send-keys`
2. The script attaches to the session — you see the built layout
3. PowerSession records everything you see on screen
4. The script also generates `demos/hero.srt` (subtitle captions)

### Step 8: End the recording

Press **Ctrl+b d** to detach from psmux, which exits the script and stops PowerSession.

Or type `exit` if the script has already detached.

### Step 9: Render the GIF

```powershell
agg demos/hero.cast demos/hero.gif --cols 120 --rows 35 --font-size 16
```

Preview: open `demos/hero.gif` in a browser or image viewer.

### Step 10: Render MP4 with subtitles (for social media)

```powershell
ffmpeg -y -i demos/hero.gif `
  -movflags faststart -pix_fmt yuv420p `
  -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2,subtitles='demos/hero.srt':force_style='FontSize=14,PrimaryColour=&H00FFFFFF,OutlineColour=&H00000000,Outline=2,MarginV=30'" `
  demos/hero.mp4
```

---

## Recording All Demos at Once

### Step 11: Use the batch recorder

```powershell
# GIFs only
pwsh -NoProfile -File demos/record-all.ps1

# GIFs + MP4 with subtitles
pwsh -NoProfile -File demos/record-all.ps1 -Mp4

# Re-render GIFs from existing recordings (no re-record)
pwsh -NoProfile -File demos/record-all.ps1 -GifOnly

# Record only demo 3
pwsh -NoProfile -File demos/record-all.ps1 -Demo 3
```

Each demo will prompt you to detach (Ctrl+b d) when done viewing.

---

## Post-Production

### Step 12: Trim the recording (optional)

If the recording has dead time at the start or end:

```powershell
# Speed up 2x
agg demos/hero.cast demos/hero-fast.gif --speed 2

# Or trim with asciinema tools if installed
# asciinema cut --start 2.0 --end 25.0 demos/hero.cast demos/hero-trimmed.cast
```

### Step 13: Edit captions (optional)

Open `demos/hero.srt` in any text editor. SRT format:

```
1
00:00:01,500 --> 00:00:04,500
psmux: tmux-compatible multiplexer for Windows

2
00:00:03,200 --> 00:00:06,200
Ctrl+b % — vertical split
```

Adjust timings, fix wording, then re-render MP4 (Step 10).

### Step 14: Upload captions to YouTube (optional)

YouTube, X (Twitter), and Reddit all accept `.srt` files as subtitle uploads. Upload the video without burned-in subtitles, then add the `.srt` as a caption track for accessibility.

---

## Outputs

After recording all demos, you'll have:

```
demos/
  hero.cast              # raw recording
  hero.gif               # README GIF
  hero.srt               # subtitle captions
  hero.mp4               # social media video (with subtitles)
  powerpack-tools.*      # same set
  neovim.*
  agent-swarm.*
  session-lifecycle.*
```

### Step 15: Add GIFs to README

```markdown
## Demo

![psmux in action](demos/hero.gif)

### Power Pack Tools
![Power Pack](demos/powerpack-tools.gif)

### Neovim Support
![Neovim](demos/neovim.gif)
```

### Step 16: Post to social media

| Platform | Format | Notes |
|----------|--------|-------|
| X (Twitter) | MP4 (< 512MB, < 2:20) | Upload `.srt` as captions |
| Reddit | GIF or MP4 | Post to r/neovim, r/windows, r/commandline |
| YouTube | MP4 | Upload `.srt` as CC track |
| Hacker News | GIF in README | Link to GitHub repo |

---

## Troubleshooting

| Problem | Fix |
|---------|-----|
| PowerSession not found | `cargo install PowerSession` (capital P and S) |
| agg not found | Download from GitHub releases (see Step 2) |
| Recording is blank | Ensure psmux is built and in PATH |
| Captions out of sync | Edit `.srt` timings, re-run ffmpeg |
| GIF too large | Reduce `--font-size`, increase `--speed`, or trim the recording |
| Terminal too small | Resize to 120x35 minimum before recording |
| psmux session conflict | `psmux kill-server` before recording |
