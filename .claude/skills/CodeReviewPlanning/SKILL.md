---
name: code-review
description: >
  Interactive code review that orchestrates deep-analysis tools (RLM, Serena, Code-Graph,
  Expert Panel, Council) to discover issues, then presents findings interactively with
  AskUserQuestion for user-driven prioritization. Use when the user asks to "review my code",
  "audit this codebase", "code health check", "what's wrong with this code", "check this
  before we change anything", "review this PR", "technical debt assessment", or any request
  that implies analyzing existing code for issues before implementing fixes. This skill is
  interactive — it pauses after each review section for user feedback and presents findings
  with concrete options.
---

# Code Review

An interactive code review workflow that orchestrates your MCP tooling (Code-Graph, Serena,
RLM, Expert Panel, Council) to discover issues, then presents them with concrete options
and tradeoffs. The user always decides what to fix.

**Review first, change never (until the user says so).**

---

## Phase 1: Discovery

Before reviewing anything, build a structural understanding using tools — not manual scanning.

### 1a. Index the Codebase

```
codegraph_index(project_root="<project>")
codegraph_stats(project_root="<project>")
```

This gives you file count, node/edge counts, and language breakdown in seconds.

### 1b. Map the Architecture

```
codegraph_components(project_root="<project>", level="module")
codegraph_diagram(project_root="<project>", scope="all", diagram_type="flowchart")
```

This auto-discovers module boundaries and generates a visual architecture diagram.

### 1c. Check for Prior Context

```
engram_recall("<project>", "code review architecture decisions")
beacon_search("<project>", "architecture")
```

Check if there are prior reviews, documented decisions, or known tech debt.

### 1d. Produce a Codebase Summary

Using the tool outputs above, write a brief summary (5-8 lines):
- Language(s), framework(s), structure
- Module count, file count, key entry points
- Test presence (look for test directories/files in the graph)
- Prior review findings (from Engram), if any

Share with the user to confirm you're looking at the right thing.

### 1e. Load Preferences

Check if the user has stated engineering preferences. If not, use defaults from
`references/preferences.md`. If they have, acknowledge how preferences change the review.

### 1f. Select Review Mode

Present via AskUserQuestion:

- **A) Comprehensive Review** — All four sections, up to 4 issues each. Best for
  major refactors, onboarding, or pre-release audits.
- **B) Focused Review** — One key finding per section. Best for quick health checks.

Make Comprehensive the recommended (first) option. Wait for the user's choice.

---

## Phase 2: Section Reviews

Work through sections **in order**. After each section, pause and ask for feedback.
Do not assume priorities on timeline or scale.

Every finding follows the format in `references/issue-format.md`:
severity label, concrete file:line references, 2-3 options with effort/risk/impact,
recommended option first, "do nothing" always valid.

---

### Section 1: Architecture Review

**Discovery tools:**

```
# Dependency graph and coupling
codegraph_dependencies(project_root="<project>", name="<module>", direction="both")

# Circular dependencies
codegraph_query(project_root="<project>",
    query="MATCH (a)-[:DEPENDS_ON]->(b)-[:DEPENDS_ON]->(a) RETURN a.name, b.name")

# Serena: validate specific boundaries
get_symbols_overview("<module_file>", depth=1)
find_referencing_symbols("<boundary_class>")
```

**What to evaluate:**

| Dimension | Tools | What to Look For |
|-----------|-------|-----------------|
| Component boundaries | `codegraph_components` | Separation of concerns, layering |
| Coupling | `codegraph_dependencies` | Circular deps, high fan-in/fan-out |
| Data flow | `codegraph_diagram` (flowchart) | Bottlenecks, unclear paths |
| Security boundaries | Serena `find_symbol` on auth | Auth boundaries, API surface |

In Comprehensive mode, surface up to 4 issues. In Focused mode, surface the single
most impactful concern.

After presenting findings via AskUserQuestion, ask: "Ready to move to Code Quality,
or discuss any of these further?"

---

### Section 2: Code Quality Review

**Discovery tools:**

```
# RLM: bulk scan for DRY violations, magic numbers, error patterns
rlm_load("<src_dir>", recursive=True, glob_pattern="*.py")
rlm_exec("""
import re
files = context.split('# === ')
# Scan for patterns: duplicated logic, bare except, TODO/FIXME, magic numbers
...
""")

# Serena: precise symbol-level analysis
find_symbol("<suspect_function>", include_body=True)

# Code-Graph: find functions with zero callers (potential dead code)
codegraph_query(project_root="<project>",
    query="MATCH (f:Function) WHERE NOT ()-[:CALLS]->(f) RETURN f.name, f.file")
```

**What to evaluate:**

| Dimension | Tools | What to Look For |
|-----------|-------|-----------------|
| DRY violations | RLM bulk scan | Repeated logic — show file + line ranges, estimate saved lines |
| Error handling | RLM pattern scan | Missing try/catch, bare except, swallowed errors |
| Dead code | Code-Graph query | Unreferenced functions/classes |
| Tech debt | RLM (TODO/FIXME scan) | Complexity hotspots, hack comments |
| Engineering calibration | Serena symbol analysis | Over-engineered or under-engineered relative to preferences |

After presenting findings, pause for feedback.

---

### Section 3: Test Review

**Discovery tools:**

```
# Code-Graph: find which source files have test coverage
codegraph_dependencies(project_root="<project>", name="test_", direction="outgoing")

# RLM: analyze test quality
rlm_load("<test_dir>", recursive=True, glob_pattern="test_*.py")
rlm_exec("""
import re
files = context.split('# === ')
for f in files[1:]:
    fname = f.split(' ===')[0].strip()
    asserts = re.findall(r'assert\w*\(', f)
    test_fns = re.findall(r'def (test_\w+)', f)
    print(f"{fname}: {len(test_fns)} tests, {len(asserts)} assertions")
    if test_fns and not asserts:
        print(f"  WARNING: tests with no assertions!")
""")

# Serena: check specific untested modules
get_symbols_overview("<untested_module>", depth=1)
```

**What to evaluate:**

| Dimension | Tools | What to Look For |
|-----------|-------|-----------------|
| Coverage gaps | Code-Graph deps | Modules/functions with no test references |
| Test quality | RLM scan | Assertion strength — tests that run but don't verify |
| Edge cases | RLM + Serena | Missing boundary/error path tests |
| Failure modes | Serena (read error handlers) | Untested error paths |

If no tests exist, flag as critical and recommend a testing strategy instead of
listing individual gaps.

After presenting findings, pause for feedback.

---

### Section 4: Performance Review

**Discovery tools:**

```
# Code-Graph: find hot paths (high fan-in functions)
codegraph_query(project_root="<project>",
    query="MATCH ()-[:CALLS]->(f:Function) WITH f, count(*) AS callers
           WHERE callers > 5 RETURN f.name, f.file, callers ORDER BY callers DESC")

# Code-Graph: impact analysis on critical paths
codegraph_impact(project_root="<project>", file_or_symbol="<hot_function>")

# RLM: scan for known performance antipatterns
rlm_load("<src_dir>", recursive=True)
rlm_exec("""
import re
patterns = {
    'N+1 queries': r'for .+ in .+:\s*\n\s+.*\.(?:query|execute|filter|get)\(',
    'Unbounded collections': r'\.(?:fetchall|find)\(\)',
    'Nested loops': r'for .+:\s*\n\s+for .+:',
}
for name, pat in patterns.items():
    matches = re.findall(pat, context, re.MULTILINE)
    if matches:
        print(f"[!] {name}: {len(matches)} occurrences")
""")
```

**What to evaluate:**

| Dimension | Tools | What to Look For |
|-----------|-------|-----------------|
| Query patterns | RLM scan | N+1, unindexed lookups, excessive round-trips |
| Hot paths | Code-Graph fan-in | High-traffic functions worth optimizing |
| Memory | RLM scan | Unbounded collections, large object retention |
| Complexity | RLM + Serena | O(n^2) algorithms, unnecessary recomputation |

Only flag issues that matter at current or near-future scale.

After presenting findings, proceed to summary.

---

## Phase 3: Validation (Optional)

For critical findings or when confidence matters, add a validation layer:

### Expert Panel Review

```
panel_review(code="<code with findings>", panel="backend", context="<findings summary>")
```

Use the appropriate panel (backend, frontend, llm-ai, embedded, etc.) for domain-specific
validation. See `panel_list()` for available panels.

### Council Consensus

```
council_review(
    diff="<findings report>",
    context="Code review findings. Validate: are these real issues or false positives?",
    include_coderabbit=True
)
```

Multi-model consensus eliminates single-model blind spots. Use when:
- Security findings need high confidence
- Architectural concerns are subjective
- Findings seem uncertain

### Confidence Tiers

| Tier | Source | Action |
|------|--------|--------|
| **Highest** | Tools + Panel + Council agree + Serena confirmed | Fix |
| **High** | Tools + Council consensus | Very likely real |
| **Medium** | Tools found it, Council disputes | Present with caveats |
| **Low** | Single tool only | Mention but don't prioritize |

---

## Phase 4: Review Summary

After all sections, generate a markdown summary:

```markdown
# Code Review Summary

## Codebase
[Brief description from Phase 1]

## Review Mode
[Comprehensive / Focused]

## Architecture Diagram
[Mermaid diagram from codegraph_diagram, if generated]

## Findings by Section

### Architecture
[Issue list with chosen options]

### Code Quality
[Issue list with chosen options]

### Tests
[Issue list with chosen options]

### Performance
[Issue list with chosen options]

## Action Items
[Ordered list of agreed-upon changes]

## Deferred / Do Nothing
[Issues where user chose no action, with reasoning]
```

Save to the working directory. Then:

```
engram_remember("<project>", type="decision",
    "Code review completed. Key findings: <summary>. Actions agreed: <list>.")
```

---

## Severity Levels

- **critical** — Bugs, security vulnerabilities, data loss risks. Must address.
- **moderate** — Design problems, significant DRY violations, missing error handling. Should address.
- **minor** — Style issues, small improvements, nice-to-haves. Address if convenient.

---

## Interaction Rules

- **Do not assume priorities on timeline or scale.** Ask, don't guess.
- **After each section, pause** and ask for feedback before moving on.
- **AskUserQuestion format**: Number issues (Issue 1, 2...), letter options (A, B, C...),
  recommended option first. See `references/issue-format.md`.
- **"Do nothing" is always valid.** Frame it constructively.
- **If the user disagrees**, accept and log their choice.
- **Don't lecture.** State problem, present options, recommend, let user decide.
- **If user says "skip to X"**, skip without complaint.
