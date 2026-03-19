# Engineering Preferences Reference

User preferences guide which issues get flagged and how recommendations are ranked.
This file documents the preference dimensions and how they influence the review.

## Default Preferences

These defaults reflect a pragmatic senior engineer who values correctness and
maintainability over speed:

| Preference | Default | Effect on Review |
|-----------|---------|-----------------|
| `dry` | important | Flag repetition aggressively — even 2 occurrences of 5+ lines |
| `testing` | thorough | Flag missing tests as moderate or critical; recommend tests for edge cases |
| `engineering_level` | enough | Flag both over-engineering AND under-engineering |
| `edge_cases` | more | Enumerate missing edge cases explicitly in findings |
| `explicitness` | high | Prefer explicit code over clever abstractions |
| `thoughtfulness_vs_speed` | thoughtfulness | Recommend the more thorough option when effort is similar |

## How Preferences Affect Recommendations

### DRY: important vs relaxed

**important (default):** Flag any duplicated logic across 2+ locations. Recommend
extraction even for small patterns (5-10 lines). In options, extraction is the
recommended option.

**relaxed:** Only flag duplication at 3+ occurrences or 20+ lines. "Do nothing"
becomes the recommended option for small duplications.

### Testing: thorough vs pragmatic

**thorough (default):** Missing tests for any public function is at least a moderate
finding. Missing edge case tests are flagged individually. Recommend integration tests
for cross-module interactions.

**pragmatic:** Only flag missing tests for critical paths (auth, payments, data mutations).
Edge case tests are suggested but not flagged as issues.

### Engineering Level: enough vs robust vs minimal

**enough (default):** Flag premature abstractions (interfaces with one implementation,
factory patterns for a single type). Also flag fragile code (string parsing instead of
proper parsers, hard-coded values in business logic).

**robust:** Only flag under-engineering. Accept abstractions even if currently unused
if they serve a clear future purpose.

**minimal:** Only flag over-engineering. Accept hacky solutions if they work and are
contained.

### Edge Cases: more vs fewer

**more (default):** Enumerate specific missing edge cases: null inputs, empty collections,
boundary values, concurrent access, network failures, malformed input. Each missing
edge case is a separate point in the finding.

**fewer:** Group edge cases by category. "Missing null checks in input validation"
rather than listing each function.

### Explicitness: high vs balanced

**high (default):** Recommend explicit error types over generic catches. Prefer named
constants over magic numbers. Recommend explicit type annotations where the language
supports them.

**balanced:** Accept implicit behavior when it follows strong conventions (e.g., Rails
convention over configuration).

## Overriding Preferences

Users can override preferences by stating them in conversation:

- "I don't care much about DRY for this prototype" → set `dry: relaxed`
- "Testing is critical for this project" → set `testing: thorough` (already default)
- "Keep it simple, we're moving fast" → set `engineering_level: minimal`, `edge_cases: fewer`
- "This is a production financial system" → set everything to maximum strictness

When the user states a preference, acknowledge it and explain how it changes the review
before proceeding.
