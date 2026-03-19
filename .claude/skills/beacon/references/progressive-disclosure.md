# Progressive Disclosure Patterns

## The Problem

LLM context windows are finite. Every token spent on documentation is a token
NOT spent on the actual task, the relevant code, or the user's instructions.

## The Solution: Progressive Disclosure

Start with a small, stable map. Drill into specifics only when needed.

## Pattern 1: The Map Entry Point

**AGENTS.md** (~100 lines) is the ONLY doc guaranteed to be in context.
Everything else is fetched on demand.

```
AGENTS.md says:     "Frontend architecture → docs/FRONTEND.md"
Agent reads:         AGENTS.md (100 lines)
Agent needs frontend: reads docs/FRONTEND.md (maybe 200 lines)
Total context used:  300 lines (not 2000+)
```

## Pattern 2: Index Files as Navigators

Each `docs/` subdirectory has an `index.md` that catalogues its contents
with one-line descriptions. The agent reads the index, then reads only
the specific doc it needs.

```
docs/design-docs/index.md says:
  "ADR-001: Auth strategy → auth-strategy.md (active)"
  "ADR-002: Database choice → database-choice.md (active)"
  "ADR-003: Old caching approach → caching-v1.md (archived)"

Agent working on auth reads only: index.md + auth-strategy.md
```

## Pattern 3: Execution Plans as Working Memory

Active execution plans serve as the agent's working memory across sessions.
Instead of re-deriving the plan each time, the agent reads the plan,
picks up where it left off, and logs progress.

```
Session 1: Agent creates plan, completes steps 1-3, logs progress
Session 2: Agent reads plan, sees steps 1-3 done, continues from step 4
Session 3: Agent reads plan, all steps done, moves to completed/
```

## Pattern 4: Generated Docs as Ground Truth

Auto-generated docs from code are always accurate by construction.
Agents can trust them without verification, unlike human-written docs
that may have drifted.

```
Agent needs to know DB schema:
  reads docs/generated/db-schema.md  ← always accurate
  NOT: asks the human or guesses from model files
```

## Anti-Pattern: Bulk Loading

```
# WRONG — loads everything into context
Read AGENTS.md
Read ARCHITECTURE.md
Read docs/FRONTEND.md
Read docs/DESIGN.md
Read docs/SECURITY.md
Read docs/RELIABILITY.md
Read docs/QUALITY_SCORE.md
... (1500+ lines consumed before task even starts)

# RIGHT — progressive disclosure
Read AGENTS.md (100 lines)
Task is about frontend auth → read docs/FRONTEND.md + docs/SECURITY.md
(300 lines total, highly relevant)
```
