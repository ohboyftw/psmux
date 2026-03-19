# Issue Presentation Format

This reference defines the exact format for presenting code review findings.
Every issue follows this structure — no exceptions.

## Template

```
### Issue {NUMBER} ({SEVERITY}): {SHORT_TITLE}

**Location:** `{file_path}` lines {start}-{end}
{Additional locations if the issue spans multiple files}

**Problem:** {1-3 sentence concrete description of what's wrong and why it matters.
Include specific code references, not vague statements.}

**Options:**

**A) {RECOMMENDED_OPTION_TITLE} (recommended)**
- Effort: {low / medium / high}
- Risk: {low / medium / high} — {one sentence on what could go wrong}
- Impact: {what other code is affected}
- Maintenance: {ongoing burden}
- Why recommended: {1 sentence tying back to user preferences}

**B) {ALTERNATIVE_OPTION_TITLE}**
- Effort: {low / medium / high}
- Risk: {low / medium / high} — {one sentence}
- Impact: {what other code is affected}
- Maintenance: {ongoing burden}

**C) Do nothing**
- Risk: {what happens if this is left as-is}
- When this is fine: {circumstances where ignoring this is acceptable}
```

## AskUserQuestion Format

After presenting the issue details in prose, use AskUserQuestion with options formatted as:

```
"Issue 1 — A) Extract shared validator (recommended)"
"Issue 1 — B) Inline with comments"
"Issue 1 — C) Do nothing"
```

When presenting multiple issues in a single AskUserQuestion (Comprehensive mode),
each question maps to one issue:

```
Question 1: "Issue 1 (critical): Missing auth check on /api/admin"
  Options: ["A) Add middleware guard (recommended)", "B) Add inline check", "C) Do nothing"]

Question 2: "Issue 2 (moderate): Duplicated validation in user.js and admin.js"
  Options: ["A) Extract to shared validator (recommended)", "B) Keep separate", "C) Do nothing"]
```

Limit to 3 questions per AskUserQuestion call. If you have 4 issues, use two calls
(3 + 1) rather than cramming them all in.

## Severity Guidelines

### Critical
- Active bugs that produce wrong results
- Security vulnerabilities (auth bypass, injection, secrets in code)
- Data loss or corruption risks
- Race conditions that affect correctness

Flag these even in Focused mode.

### Moderate
- Significant DRY violations (3+ occurrences or 20+ duplicated lines)
- Missing error handling on external calls (network, file I/O, DB)
- Architectural coupling that will block future changes
- Missing tests for critical business logic
- Performance issues at current scale

### Minor
- Style inconsistencies
- Small DRY violations (2 occurrences, <10 lines)
- Missing tests for utility functions
- Theoretical performance concerns at scales the project won't reach soon
- Over-engineering that isn't actively causing problems

## Examples

### Good Issue Presentation

**Issue 1 (critical): Unvalidated user input in SQL query**

**Location:** `src/db/queries.js` lines 45-52

**Problem:** The `getUserById` function interpolates the `id` parameter directly into
the SQL string without parameterization. This is a SQL injection vulnerability that
allows any authenticated user to execute arbitrary queries.

**Options:**

**A) Use parameterized queries (recommended)**
- Effort: low — change 1 function, ~5 lines
- Risk: low — parameterized queries are well-understood
- Impact: none — function signature stays the same
- Maintenance: none — this is the standard approach
- Why recommended: eliminates a critical security vulnerability with minimal effort

**B) Add input sanitization**
- Effort: low — add regex validation before query
- Risk: medium — sanitization is easy to get wrong, bypasses are common
- Impact: none
- Maintenance: medium — must update sanitization rules as input formats change

**C) Do nothing**
- Risk: high — SQL injection on an authenticated endpoint
- When this is fine: never for a production system

---

### Bad Issue Presentation (don't do this)

"There might be some SQL injection issues. You should probably use parameterized queries.
Also the code could be more DRY in a few places."

This is bad because: no file references, no severity, no options, no concrete description,
multiple unrelated issues lumped together, and no clear recommendation.

## Effort Scale

- **Low**: < 30 minutes, < 20 lines changed, single file, no new dependencies
- **Medium**: 1-4 hours, 20-100 lines, 2-5 files, may need new utility/helper
- **High**: 4+ hours, 100+ lines, multiple files, possible new abstractions or dependencies

## Risk Scale

- **Low**: Well-understood change, easily reversible, good test coverage exists
- **Medium**: Some uncertainty, touches shared code, tests may need updating
- **High**: Significant refactor, many callers affected, could introduce regressions
