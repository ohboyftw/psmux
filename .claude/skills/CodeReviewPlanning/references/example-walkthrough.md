# Example Walkthrough

Shows how a Comprehensive Review flows end-to-end with MCP tool integration.

## Phase 1: Discovery

**Claude indexes and maps the codebase:**

```
codegraph_index(project_root="/app")
codegraph_stats(project_root="/app")
# -> 38 files, 1,016 nodes, 1,420 edges, TypeScript

codegraph_components(project_root="/app", level="module")
# -> routes, services, models, middleware, utils

codegraph_diagram(project_root="/app", scope="all", diagram_type="flowchart")
# -> Mermaid diagram showing module relationships

engram_recall("/app", "code review")
# -> No prior reviews found
```

**Claude shares the summary:**

> **Codebase Summary**
> - Language: TypeScript / Node.js
> - Framework: Express.js with Prisma ORM
> - Structure: 5 modules (routes, services, models, middleware, utils)
> - Graph: 1,016 nodes, 1,420 edges across 38 files (~4,200 LOC)
> - Tests: `__tests__/` with 12 test files (Jest)
> - CI: GitHub Actions, ESLint configured
> - No prior code reviews on record
>
> Does this look right? Anything I should focus on or skip?

**Claude asks for mode selection via AskUserQuestion:**

```
Question: "How thorough should this review be?"
Options:
  - "A) Comprehensive -- 4 sections, up to 4 issues each (recommended)"
  - "B) Focused -- 4 sections, 1 key issue each"
```

User selects A.

## Section 1: Architecture Review

**Claude runs discovery:**

```
# Check for circular dependencies
codegraph_query(project_root="/app",
    query="MATCH (a)-[:DEPENDS_ON]->(b)-[:DEPENDS_ON]->(a) RETURN a.name, b.name")
# -> auth.service <-> user.service

# Check coupling on routes
codegraph_dependencies(project_root="/app", name="routes/users", direction="outgoing")
# -> Direct deps on Prisma (bypassing services)

# Validate with Serena
find_referencing_symbols("PrismaClient", relative_path="src/routes/")
# -> 4 direct Prisma calls in route handlers
```

**Claude presents findings:**

> ### Architecture Review
>
> I found 3 architectural concerns.
>
> **Issue 1 (moderate): Service layer bypassed in 4 routes**
>
> **Location:** `src/routes/users.ts` lines 23, 45, 67, 89
>
> **Problem:** Routes call Prisma directly instead of going through
> `src/services/userService.ts`. Business logic (validation, authorization)
> is duplicated in route handlers.
>
> **A) Route all DB access through services (recommended)**
> - Effort: medium -- refactor 4 route handlers
> - Risk: low -- services already exist
> - Why: Eliminates duplication, enforces boundaries
>
> **B) Add ESLint rule to prevent direct Prisma imports in routes**
> - Effort: low
> - Risk: low -- doesn't fix existing violations
>
> **C) Do nothing**
> - When fine: if these routes rarely change

**Then uses AskUserQuestion (max 3 per call):**

```
Question 1: "Issue 1 (moderate): Service layer bypassed in routes"
Options: ["A) Route through services (recommended)", "B) Add lint rule", "C) Do nothing"]

Question 2: "Issue 2 (moderate): No error handling middleware"
Options: ["A) Add centralized error handler (recommended)", "B) Add try/catch per route", "C) Do nothing"]

Question 3: "Issue 3 (minor): Circular dependency auth <-> user services"
Options: ["A) Extract shared types (recommended)", "B) Merge into single service", "C) Do nothing"]
```

**Claude pauses:** "Those are my architecture findings. Ready for Code Quality?"

## Sections 2-4

Same pattern. Key behaviors:

- Each section uses the appropriate discovery tools (RLM for bulk, Serena for precision,
  Code-Graph for structure)
- Issues presented in full detail, then options via AskUserQuestion
- Claude pauses between sections
- If user says "skip to performance" -- Claude skips without complaint
- If user says "go deeper on Issue 2" -- Claude uses Serena/RLM for more detail

## Phase 3: Validation (optional)

If the user wants high confidence on critical findings:

```
panel_review(code="<route handler code>", panel="backend",
    context="Express.js API, found direct DB calls bypassing service layer")
# -> Backend experts confirm: service bypass is a real concern

council_review(diff="<findings report>",
    context="Validate these code review findings",
    include_coderabbit=True)
# -> 4/5 models agree on service bypass; 3/5 flag missing rate limiting (new finding!)
```

## Phase 4: Summary

```markdown
# Code Review Summary

## Codebase
TypeScript/Express.js API with Prisma ORM, ~4,200 LOC, 5 modules

## Review Mode
Comprehensive

## Findings

### Architecture (3 issues)
1. (moderate) Service layer bypassed -> **Chosen: A) Route through services**
2. (moderate) No error handling middleware -> **Chosen: A) Add centralized handler**
3. (minor) Circular auth<->user dependency -> **Chosen: C) Do nothing**

### Code Quality (4 issues)
1. (critical) Hardcoded JWT secret -> **Chosen: A) Move to env vars**
2. (moderate) Validation duplicated in 3 places -> **Chosen: A) Extract shared validator**
3. (moderate) Inconsistent error types -> **Chosen: B) Standardize gradually**
4. (minor) Magic numbers in pagination -> **Chosen: A) Extract to constants**

### Tests (2 issues)
1. (moderate) No tests for auth middleware -> **Chosen: A) Add unit tests**
2. (moderate) Missing edge cases in user service -> **Chosen: A) Add tests**

### Performance (1 issue)
1. (moderate) N+1 in getUserWithPosts -> **Chosen: A) Add Prisma include**

## Action Items (by priority)
1. Move JWT secret to environment variables (critical, low effort)
2. Add centralized error handling middleware (moderate, medium effort)
3. Route DB access through service layer (moderate, medium effort)
4. Extract shared validation (moderate, medium effort)
5. Add auth middleware tests (moderate, medium effort)
6. Add edge case tests (moderate, medium effort)
7. Fix N+1 query (moderate, low effort)
8. Standardize error types (moderate, ongoing)
9. Extract pagination constants (minor, low effort)

## Deferred
- Circular auth<->user dependency (minor, not causing problems)
```

**Claude stores the review:**

```
engram_remember("/app", type="decision",
    "Code review: 10 findings across 4 sections. 9 action items agreed.
     Critical: JWT secret hardcoded. Key pattern: service layer bypass in routes.")
```

## Anti-patterns

- **Don't dump all sections at once.** Pause between sections.
- **Don't present more than 3 issues per AskUserQuestion.** Split into multiple calls.
- **Don't skip the codebase summary.** User needs to confirm scope.
- **Don't assume the user wants to fix everything.** "Do nothing" is valid.
- **Don't generate code.** This is a review, not a fix.
- **Don't manually scan files.** Use Code-Graph, RLM, and Serena.
