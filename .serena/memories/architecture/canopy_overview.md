# Canopy — Sibling Project Overview

Canopy is a local-first, multi-provider agent orchestration platform. Sibling project to psmux at D:\Home\canopy\.

## Key Facts
- **What**: Watches task backlogs (GitHub Issues, PRD files, markdown checkboxes), decomposes work, routes to Pi (simple) or Claude Code (complex), spawns in psmux panes + git worktrees, monitors health, delivers results (PRs or checkbox commits)
- **Audience**: Power user tool for Aravind's personal workflow
- **Tech stack**: Python async CLI (Phase 1), standalone process in psmux pane
- **Runtime dependency**: psmux on PATH, gh CLI, Python 3.11+
- **Design principle**: "Match the source" — input format dictates output, communication channel, delivery
- **Trust dial**: Level 1 (supervised) -> Level 2 (checkpoints) -> Level 3 (autonomous)
- **Intelligence**: Hybrid — Haiku for refinement/routing, escalate ambiguous tasks to human
- **Persistence**: Single canopy-state.json + psmux session persistence + git worktrees
- **Spec**: D:\Home\canopy\docs\specs\2026-03-17-canopy-design.md

## Relationship to psmux
- Sibling project, not a psmux feature
- Canopy calls psmux as subprocess (split-window, send-keys, capture-pane, list-panes)
- psmux has no knowledge of canopy
- psmux P0 tasks (JSON output) would benefit canopy but not blockers
