---
name: beacon
description: >
  Maintains a structured system of record for any codebase using the Beacon pattern —
  a lightweight AGENTS.md map with progressive disclosure into a versioned docs/ knowledge base.
  Use when: initializing a new project's knowledge base, creating or updating execution plans,
  running doc-gardening checks, scaffolding AGENTS.md, or when Claude Code needs to orient
  itself in a repo before starting work. Also use when the user says "init docs", "update plans",
  "doc health", "gardening", or references keeping documentation in sync with code.
---

# Beacon Knowledge System

## Philosophy

Context is a scarce resource. Don't give agents an encyclopedia — give them a map.

This skill implements the **Beacon pattern**: a short, stable entry point (`AGENTS.md`, ~100 lines)
that serves as a table of contents pointing to a structured `docs/` directory that is the
system of record. Agents start small and drill deeper only when needed (progressive disclosure).

**Why this works:**
- A giant instruction file crowds out the task, the code, and relevant docs
- When everything is "important," nothing is — agents pattern-match locally instead of navigating intentionally
- Monolithic docs rot instantly and are impossible to verify mechanically
- Structured docs enable linting, freshness tracking, ownership, and cross-link validation

---

## Modes of Operation

| Mode | Trigger | What it does |
|------|---------|--------------|
| **Init** | `init docs`, new project setup | Scaffold full knowledge base structure |
| **Orient** | Start of any task, `orient`, `where am I` | Read AGENTS.md, identify relevant docs for current task |
| **Plan** | `create plan`, `plan <feature>` | Create execution plan in `docs/exec-plans/active/` |
| **Update** | `update docs`, after completing work | Update relevant docs to reflect code changes |
| **Garden** | `doc health`, `gardening`, CI trigger | Scan for stale/broken/orphaned docs, propose fixes |
| **Search** | `find docs about X`, `where is Y documented` | Hybrid BM25+semantic search across the knowledge base |
| **Generate** | `generate docs`, after schema changes | Auto-generate docs from code (DB schema, API specs, etc.) |

---

## Knowledge Base Structure

Every project using this skill MUST have this layout:

```
project-root/
├── AGENTS.md                    # The map (~100 lines, injected into context)
├── ARCHITECTURE.md              # Top-level domain map and package layering
├── docs/
│   ├── design-docs/
│   │   ├── index.md             # Catalogue of all design docs with status
│   │   ├── core-beliefs.md      # Non-negotiable agent-first operating principles
│   │   └── <decision-name>.md   # Individual design decisions (ADR-style)
│   ├── exec-plans/
│   │   ├── active/              # Currently executing plans
│   │   ├── completed/           # Done plans (kept for context)
│   │   ├── _template.md         # Plan template
│   │   └── tech-debt-tracker.md # Known technical debt registry
│   ├── generated/               # Machine-generated docs (DO NOT edit manually)
│   │   ├── db-schema.md
│   │   ├── api-routes.md
│   │   └── dependency-graph.md
│   ├── product-specs/
│   │   ├── index.md             # Feature catalogue
│   │   └── <feature-name>.md    # Individual feature specs
│   ├── references/
│   │   └── <tool>-llms.txt      # LLM-optimized external reference docs
│   ├── DESIGN.md                # Design system and UI conventions
│   ├── FRONTEND.md              # Frontend architecture and patterns
│   ├── PLANS.md                 # Planning conventions and active plan index
│   ├── PRODUCT_SENSE.md         # Product principles and user-facing quality bar
│   ├── QUALITY_SCORE.md         # Per-domain quality grades and gap tracking
│   ├── RELIABILITY.md           # Error handling, monitoring, SLOs
│   └── SECURITY.md              # Security policies and threat model
```

---

## Document Conventions

### Frontmatter (REQUIRED on every doc)

Every `.md` file in `docs/` MUST have YAML frontmatter:

```yaml
---
title: "Human-readable title"
status: draft | active | stale | archived
owner: "<person-or-team>"
last_verified: "YYYY-MM-DD"
created: "YYYY-MM-DD"
tags: [relevant, searchable, tags]
cross_links:
  - docs/design-docs/core-beliefs.md
  - ARCHITECTURE.md
---
```

**Status lifecycle:** `draft` → `active` → `stale` (auto-detected after 30 days) → `archived`

### Writing Rules

1. **Be concise** — every sentence must earn its place
2. **Link, don't duplicate** — reference other docs, never copy content between files
3. **Use concrete examples** — abstract principles must include at least one concrete example
4. **Version decisions** — record why, not just what (include rejected alternatives)
5. **Machine-parseable structure** — use consistent heading levels so linters can validate

---

## Mode: Init

**When:** Setting up a new project or adding Beacon to an existing repo.

### Workflow

1. **Scan the project** to understand what exists:
   ```bash
   # Check for existing docs
   find . -name "*.md" -not -path "*/node_modules/*" | head -30
   # Check project structure
   ls -la
   # Check for package files, config, etc.
   cat package.json 2>/dev/null || cat Cargo.toml 2>/dev/null || cat pyproject.toml 2>/dev/null || echo "No standard project file found"
   ```

2. **Create the directory structure:**
   ```bash
   mkdir -p docs/{design-docs,exec-plans/active,exec-plans/completed,generated,product-specs,references}
   ```

3. **Generate AGENTS.md** from the template at `templates/AGENTS.md.template`
   - Customize domain table based on actual project structure
   - Keep it under 100 lines
   - Every line must be a pointer, not an explanation

4. **Generate ARCHITECTURE.md** from the template at `templates/ARCHITECTURE.md.template`
   - Map actual domains, packages, and service boundaries
   - Include dependency direction (what depends on what)

5. **Generate core documents:**
   - `docs/design-docs/index.md` — empty catalogue, ready for entries
   - `docs/design-docs/core-beliefs.md` — from template, customized to project
   - `docs/exec-plans/_template.md` — execution plan template
   - `docs/exec-plans/tech-debt-tracker.md` — empty tracker
   - `docs/QUALITY_SCORE.md` — initial quality grades (all "ungraded")

6. **Generate generated docs** by scanning code:
   - If database exists → `docs/generated/db-schema.md`
   - If API routes exist → `docs/generated/api-routes.md`
   - If package dependencies exist → `docs/generated/dependency-graph.md`

7. **Create `.github/workflows/doc-health.yml`** (or equivalent CI) from `scripts/doc-health-ci.yml`

8. **Verify:** Run the doc linter (`scripts/lint-docs.sh`) to confirm the new structure passes all checks.

---

## Mode: Orient

**When:** Starting any task. This is the FIRST thing to do.

### Workflow

1. **Read `AGENTS.md`** — this gives you the map
2. **Identify which domain docs are relevant** to the current task
3. **Read only those docs** — progressive disclosure, not bulk loading
4. **Check `docs/exec-plans/active/`** for related ongoing work
5. **Check `docs/exec-plans/tech-debt-tracker.md`** for known issues in the area
6. **Proceed with task** using the context gathered

**Critical rule:** NEVER skip orientation. Even for "simple" tasks, read AGENTS.md first.
An agent that doesn't orient is an agent that will duplicate work, violate conventions,
or miss critical constraints.

---

## Mode: Plan

**When:** Starting complex work that spans multiple files or sessions.

### Workflow

1. **Determine plan scope:**
   - Small change (1-3 files, single session) → ephemeral plan in PR description, no file needed
   - Complex work (4+ files, multi-session, or involves decisions) → full execution plan

2. **Create execution plan** from `templates/exec-plan.md.template`:
   ```bash
   cp docs/exec-plans/_template.md "docs/exec-plans/active/<descriptive-name>.md"
   ```

3. **Fill in the plan:**
   - Objective (1-2 sentences)
   - Success criteria (testable assertions)
   - Steps with checkboxes
   - Decision log (append decisions as they're made)
   - Risks and mitigations
   - Links to relevant design docs and specs

4. **Update `docs/PLANS.md`** to reference the new active plan

5. **As work progresses:**
   - Check off completed steps
   - Log decisions with rationale
   - Note any deviations from the plan

6. **When complete:**
   - Move from `active/` to `completed/`
   - Update `docs/PLANS.md`
   - Record any new tech debt in `docs/exec-plans/tech-debt-tracker.md`

---

## Mode: Update

**When:** After completing work that changes behavior, architecture, or interfaces.

### Workflow

1. **Identify which docs are affected** by the changes:
   - Did you change architecture? → Update `ARCHITECTURE.md`
   - Did you change APIs? → Update `docs/generated/api-routes.md` or trigger regeneration
   - Did you change database schema? → Regenerate `docs/generated/db-schema.md`
   - Did you add a new feature? → Create or update spec in `docs/product-specs/`
   - Did you make a design decision? → Create ADR in `docs/design-docs/`
   - Did you fix tech debt? → Update `docs/exec-plans/tech-debt-tracker.md`

2. **Update frontmatter** on all touched docs:
   - Set `last_verified` to today
   - Verify `cross_links` are still valid
   - Update `status` if applicable

3. **Update AGENTS.md** if:
   - A new domain was added
   - Quality grades changed
   - The pointer structure needs adjustment

4. **Update quality grades** in `docs/QUALITY_SCORE.md` if applicable

5. **Run the linter** to verify all cross-links resolve and structure is valid

---

## Mode: Search

**When:** Agent needs to find specific information across the knowledge base without
reading every document. Also used for Orient mode when the task domain isn't obvious.

### Retrieval Architecture

The knowledge base has a Python retrieval engine (`scripts/beacon_index/`) that provides:

1. **Markdown-aware chunking** — splits docs by headings, code blocks, tables, and
   paragraph boundaries. Each chunk retains its heading hierarchy and frontmatter.
2. **BM25 sparse search** (zero dependencies) — exact keyword matching with bigram
   support, frontmatter field boosting (title × 2.0, tags × 1.5), and heading boost.
3. **Semantic vector search** (optional) — dense embeddings via `all-MiniLM-L6-v2`
   for conceptual matching ("how does auth work" → security docs).
4. **Hybrid fusion** — Reciprocal Rank Fusion (RRF) merges BM25 and semantic results.
   Exact matches AND conceptual matches both surface.

### Setup

```bash
# Core (BM25 only — zero dependencies beyond Python stdlib + PyYAML):
pip install pyyaml --break-system-packages

# Full (BM25 + Semantic):
pip install pyyaml sentence-transformers numpy --break-system-packages
```

### Workflow

1. **Build index** (run once, rebuild after doc changes):
   ```bash
   python scripts/beacon_cli.py index /path/to/project
   ```
   This creates `.beacon/` directory with the saved index.

2. **Search:**
   ```bash
   python scripts/beacon_cli.py search /path/to/project "database migration strategy"
   python scripts/beacon_cli.py search /path/to/project "auth" --doc docs/SECURITY.md
   python scripts/beacon_cli.py search /path/to/project "schema" --type code
   ```

3. **Stats:**
   ```bash
   python scripts/beacon_cli.py stats /path/to/project
   ```

### Programmatic Usage (for agents)

```python
from beacon_index.retriever import HybridRetriever

retriever = HybridRetriever("/path/to/project")
retriever.build_index()

# Search returns ranked results with context
results = retriever.search("how does authentication work", top_k=5)
for r in results:
    print(r.chunk.context_header)  # [docs/SECURITY.md] Auth > OAuth Flow (lines 42-58)
    print(r.score)                  # 0.847
    print(r.source)                 # "hybrid" | "bm25" | "semantic"
    print(r.chunk.content)          # The actual text

# Formatted output for agent consumption
print(retriever.search_formatted("database schema", top_k=3))
```

### How the Agent Should Use Search

During **Orient** mode, if the task domain isn't obvious from AGENTS.md:
1. Build/load the index
2. Search with the task description as query
3. Read only the top 2-3 matching documents
4. Proceed with task

This replaces the anti-pattern of reading every domain doc. Search is how the
agent navigates from the map to the right specific doc.

---

## Mode: Garden (Python)

**When:** Periodic maintenance, CI check, or explicit user request.

The Python gardener (`scripts/beacon_index/gardener.py`) replaces the bash linter
with richer capabilities:

### Standard Checks (fast)

| Check | What it catches |
|-------|-----------------|
| **Structure** | Missing dirs, files, templates |
| **AGENTS.md size** | Over 100 lines |
| **Frontmatter** | Missing/invalid YAML frontmatter fields |
| **Freshness** | `last_verified` older than 30 days |
| **Cross-links** | Broken links (inline markdown + frontmatter cross_links) |
| **Orphans** | Docs not referenced by any index or AGENTS.md |
| **Index files** | Directories without `index.md` |
| **Plan hygiene** | Active plans with no updates in 14 days |

### Deep Checks (uses BM25, slower)

| Check | What it catches |
|-------|-----------------|
| **Near-duplicates** | Content with >70% Jaccard token similarity across docs |
| **Cross-link suggestions** | Missing cross-links between related documents |

### Usage

```bash
# Standard check
python scripts/beacon_cli.py garden /path/to/project

# Deep check with auto-fix
python scripts/beacon_cli.py garden /path/to/project --deep --fix

# JSON output for CI
python scripts/beacon_cli.py garden /path/to/project --json -o report.json
```

### Auto-Fix Rules

The gardener will auto-fix (with `--fix`):
- Set `status: stale` on docs past freshness threshold
- Create placeholder `index.md` files for directories that lack them

Everything else is flagged for human review.

### CI Integration

```yaml
# In .github/workflows/doc-health.yml
- name: Run doc gardener
  run: |
    pip install pyyaml --break-system-packages
    python scripts/beacon_cli.py garden . --json -o report.json
    python scripts/beacon_cli.py garden .  # Human-readable output
```

---

## Mode: Generate

**When:** After schema changes, API changes, or dependency updates.

### Supported generators

| Generator | Source | Output |
|-----------|--------|--------|
| DB Schema | Prisma, SQLAlchemy, Drizzle, raw SQL | `docs/generated/db-schema.md` |
| API Routes | Express, FastAPI, Next.js routes | `docs/generated/api-routes.md` |
| Dependencies | package.json, requirements.txt, Cargo.toml | `docs/generated/dependency-graph.md` |

### Workflow

1. **Detect the project's tech stack** (check package files, framework config)
2. **Run the appropriate generator script** from `scripts/`
3. **Write output to `docs/generated/`** with frontmatter:
   ```yaml
   ---
   title: "Database Schema"
   status: active
   owner: auto-generated
   last_verified: "2026-02-12"
   generated_from: "prisma/schema.prisma"
   generator: "scripts/generate-db-schema.sh"
   ---
   ```
4. **DO NOT edit generated files manually** — they are overwritten on regeneration

---

## Scripts Reference

All scripts live in the skill's `scripts/` directory. Copy them to the project's
`scripts/` or `.github/` directory during init.

| Script | Purpose |
|--------|---------|
| `beacon_cli.py` | Main CLI: index, search, garden, stats |
| `beacon_index/` | Python package: chunker, BM25, semantic, retriever, gardener |
| `lint-docs.sh` | Lightweight bash linter (for minimal environments without Python) |
| `doc-health-ci.yml` | GitHub Actions workflow for CI doc health checks |
| `generate-db-schema.sh` | Generate DB schema doc from ORM/SQL |
| `generate-api-routes.sh` | Generate API routes doc from framework |

---

## Templates Reference

All templates live in the skill's `templates/` directory.

| Template | Used for |
|----------|----------|
| `AGENTS.md.template` | Initial AGENTS.md generation |
| `ARCHITECTURE.md.template` | Initial architecture doc |
| `exec-plan.md.template` | New execution plans |
| `design-decision.md.template` | Architecture Decision Records (ADRs) |
| `core-beliefs.md.template` | Project's non-negotiable principles |
| `quality-score.md.template` | Per-domain quality tracking |
| `product-spec.md.template` | Feature specification |

---

## AGENTS.md Rules

The AGENTS.md file is THE critical artifact. Rules:

1. **Maximum 100 lines** (hard limit, warn at 80)
2. **Every line is a pointer** — no explanations, no tutorials
3. **Include a domain table** with quality grades and doc links
4. **Include "before you start" checklist** pointing to orientation steps
5. **Include "where to look" section** organized by task type
6. **Never put code examples in AGENTS.md** — link to docs that contain them
7. **Review AGENTS.md on every PR** that touches docs/ structure

---

## Quality Score Tracking

`docs/QUALITY_SCORE.md` grades each domain on a letter scale:

| Grade | Meaning |
|-------|---------|
| A | Well-documented, tested, no known gaps |
| B | Documented, minor gaps or stale sections |
| C | Partially documented, significant gaps |
| D | Minimal documentation, major gaps |
| F | Undocumented or documentation is actively misleading |
| — | Ungraded (new domain) |

Track changes over time by appending dated entries. This creates accountability
and shows documentation progress/regression.

---

## Anti-Patterns (Things to Avoid)

1. **The encyclopedia AGENTS.md** — If AGENTS.md is over 100 lines, it's a manual, not a map
2. **Orphaned docs** — Every doc must be reachable from AGENTS.md within 2 hops
3. **Stale generated docs** — If generated docs don't match code, they're worse than no docs
4. **Plans without decisions** — An execution plan without a decision log is just a todo list
5. **Copy-pasted content** — If the same info exists in two docs, one will rot. Link instead.
6. **Unowned docs** — Every doc needs an owner. No owner = no one responsible for freshness.

---

## Important Notes

- **AGENTS.md is the most important file** — it's the only doc guaranteed to be in context
- **Progressive disclosure is non-negotiable** — never bulk-load all docs into context
- **Freshness over completeness** — a short, accurate doc beats a comprehensive stale one
- **Mechanical enforcement prevents rot** — always set up CI checks, never rely on discipline alone
- **Plans are first-class artifacts** — treat them with the same rigor as code
- **The doc-gardener is your ally** — run it often, trust its output, fix what it finds
