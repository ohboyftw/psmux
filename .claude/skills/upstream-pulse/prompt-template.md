# upstream-pulse Analysis Prompt

You are analyzing upstream changes for the psmux project's `ohboy-builds` branch.

## Context

psmux is a Windows-native terminal multiplexer (tmux alternative) built in Rust.
The `ohboy-builds` branch is a feature fork that adds:
- CustomPaneBackend (JSON-RPC named pipe server for Claude Code's TeammateTool)
- Remote tmux control mode (SSH + `-CC` parser)
- DCS passthrough (VT parser forwarding)
- Session resurrection snapshots
- Agent orchestration features (warm pool, wait-pane, @agent metadata)
- Claude Code hooks integration

Changes from three sources need to be evaluated for their impact on ohboy-builds.

## Integration Points to Watch

### psmux/psmux upstream
- IPC protocol changes (named pipes, TCP)
- ConPTY / Windows Console API changes
- Pane lifecycle (create, kill, resize, focus)
- Configuration parsing (`set -g` syntax)
- Key binding system
- Copy mode / scrollback buffer

### Claude Code
- TeammateTool protocol (tmux spawn backend contract)
- `CLAUDE_PANE_BACKEND_SOCKET` env var usage
- Agent spawning flow (pane lifecycle expectations)
- `send-keys` / `capture-pane` / `list-panes` output format
- Any tmux compatibility requirements

### Pi coding agent
- CLI flags and invocation patterns (affects `pi-dispatch.ps1`)
- Output format (stdout, marker files, exit codes)
- Extension API (affects `pi-swarm.ps1`)
- Headless mode behavior

## Tier Rubric

Assign each change to exactly one tier:

### Tier 1: MERGES
- Breaking protocol changes (IPC, JSON-RPC, TeammateTool contract)
- API removals or signature changes we depend on
- Security fixes that MUST be applied
- Changes to files we've diverged on that will cause merge conflicts

### Tier 2: QUICK CONFIG
- New configuration options (1-10 line port)
- New CLI flags we should recognize
- Default value changes
- Environment variable additions

### Tier 3: LOW COMPLEXITY / HIGH IMPACT
- Performance fixes (ConPTY, rendering, event loop)
- Stability improvements we benefit from
- Bug fixes in code paths we share
- Changes under 50 lines that improve reliability

### Tier 4: FIXES
- Bug fixes in code paths we've diverged from
- Edge case handling we may or may not need
- Test improvements
- Documentation fixes

### Tier 5: FEATURES
- New commands we could port
- UI enhancements (status bar, borders, popups)
- New integration points
- Quality-of-life improvements

## Output Format

For each change item, output:

```
- **Tier N** | source: one-line summary
  - Reasoning: why this tier
  - Action: what to do in ohboy-builds (merge, cherry-pick, port, ignore, verify)
```

Group by tier (1 first, 5 last). Within each tier, group by source.
If a tier has no items, show "(none)".
