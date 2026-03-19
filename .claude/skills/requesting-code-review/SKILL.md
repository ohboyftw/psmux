---
name: requesting-code-review
description: >
  Use when completing tasks, implementing major features, or before merging.
  Combines superpowers:code-reviewer subagent with LLM Council ensemble review
  for comprehensive multi-perspective code validation.
allowed-tools: Bash, Read
---

# Requesting Code Review

Two-layer code review: a focused subagent review plus an optional multi-LLM
council ensemble. Use the subagent for every review; add the council for
high-stakes changes.

**Core principle:** Review early, review often. Escalate to council when it matters.

## When to Request Review

**Mandatory (subagent review):**
- After each task in subagent-driven development
- After completing a major feature
- Before merge to main

**Add Council Review when:**
- Changes touch security-sensitive code (auth, crypto, input validation)
- Architectural changes (new modules, new patterns, API surface changes)
- Before merge to main on shared/production branches
- Complex refactors affecting multiple subsystems

**Optional but valuable (subagent only):**
- When stuck (fresh perspective)
- Before refactoring (baseline check)
- After fixing a complex bug

---

## Layer 1: Subagent Code Review

Standard single-reviewer analysis via the superpowers:code-reviewer subagent.

### Step 1: Get git SHAs

```bash
BASE_SHA=$(git rev-parse HEAD~1)  # or origin/main for full branch diff
HEAD_SHA=$(git rev-parse HEAD)
```

### Step 2: Dispatch code-reviewer subagent

Use the Task tool with `superpowers:code-reviewer` type. Fill the template
placeholders:

- `{WHAT_WAS_IMPLEMENTED}` - What you just built
- `{PLAN_OR_REQUIREMENTS}` - What it should do
- `{BASE_SHA}` - Starting commit
- `{HEAD_SHA}` - Ending commit
- `{DESCRIPTION}` - Brief summary

### Step 3: Act on feedback

- Fix **Critical** issues immediately
- Fix **Important** issues before proceeding
- Note **Minor** issues for later
- Push back if the reviewer is wrong (with reasoning)

---

## Layer 2: Multi-LLM Council Review (Optional)

Fan out the diff to multiple LLM providers for independent analysis, then
synthesize consensus findings. This catches blind spots that a single model
might miss.

### Prerequisites

Check council availability first:

```
council_status
```

This shows which providers have API keys configured and will participate.
At minimum, one provider must be active.

### Step 1: Get the diff

```bash
DIFF=$(git diff BASE_SHA...HEAD_SHA)
```

For large diffs, consider scoping to changed files only:
```bash
DIFF=$(git diff BASE_SHA...HEAD_SHA -- path/to/relevant/files/)
```

### Step 2: Run council review

Use the `council_review` MCP tool:

```
council_review(
  diff: "<the diff text>",
  context: "Project: <name>. Language: <lang>. This change implements <summary>. Key concerns: <any specific areas to focus on>."
)
```

The council will:
1. Fan out to all configured LLM providers in parallel
2. Each provider reviews independently (security, correctness, architecture, style)
3. Synthesize consensus across all reviewers
4. Optionally merge with CodeRabbit analysis for grand synthesis

### Step 3: Interpret council results

The council report includes:
- **Per-model findings** - What each LLM flagged independently
- **Consensus issues** - Problems flagged by multiple models (highest confidence)
- **Unique findings** - Issues only one model caught (worth investigating)
- **Synthesis** - Overall assessment combining all perspectives

**Prioritize consensus issues** - if 3+ models flag the same problem, it is
almost certainly real. Unique findings may be false positives but deserve a
quick check.

---

## Combined Workflow

For high-stakes reviews, run both layers:

```
1. Get SHAs
2. Dispatch superpowers:code-reviewer subagent  (Layer 1)
3. While subagent runs, start council_review     (Layer 2)
4. Collect both results
5. Cross-reference:
   - Issues flagged by BOTH reviewer and council = high priority
   - Issues flagged by council consensus only = investigate
   - Issues flagged by single council member only = quick check
6. Fix all Critical/Important issues
7. Proceed or re-review
```

Running layers in parallel saves time since they are independent.

---

## Available Council Tools

| Tool | Purpose |
|------|---------|
| `council_review` | Run multi-LLM ensemble review on a diff |
| `council_status` | Check which providers are configured and active |
| `council_benchmark` | Run evaluation harness (for calibrating council accuracy) |

---

## Example

```
[Just completed: Add OAuth 2.1 token refresh to auth module]

BASE_SHA=$(git merge-base origin/main HEAD)
HEAD_SHA=$(git rev-parse HEAD)
DIFF=$(git diff $BASE_SHA...$HEAD_SHA)

--- Layer 1: Subagent Review ---
[Dispatch superpowers:code-reviewer]
  WHAT_WAS_IMPLEMENTED: OAuth 2.1 token refresh with PKCE
  PLAN_OR_REQUIREMENTS: Auth module spec in docs/auth-design.md
  BASE_SHA: a7981ec
  HEAD_SHA: 3df7661
  DESCRIPTION: Added token refresh, PKCE challenge, and token revocation

--- Layer 2: Council Review (security-sensitive change) ---
council_review(
  diff: DIFF,
  context: "Python FastAPI project. OAuth 2.1 implementation.
            Focus: token security, PKCE correctness, timing attacks."
)

--- Results ---
Subagent: Ready with fixes (missing rate limiting on refresh endpoint)
Council consensus: Token storage uses plaintext (3/4 models flagged)
Council unique: One model suggested constant-time comparison for tokens

[Fix rate limiting + token encryption, investigate constant-time comparison]
```

## Red Flags

**Never:**
- Skip review because "it's simple"
- Ignore Critical issues from either layer
- Proceed with unfixed Important issues
- Dismiss council consensus findings without investigation

**If reviewer is wrong:**
- Push back with technical reasoning
- Show code/tests that prove correctness
- Request clarification on ambiguous feedback

See subagent template at: `superpowers/requesting-code-review/code-reviewer.md`
