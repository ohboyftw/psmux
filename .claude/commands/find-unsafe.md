---
description: Find and audit all unsafe blocks in the codebase
---

Search the entire `src/` directory for all `unsafe` blocks. For each one:

1. Show the file, line number, and the unsafe block with surrounding context
2. Check if there is a `// SAFETY:` comment immediately above it
3. Flag any blocks missing the safety comment
4. Briefly assess whether the unsafe usage looks correct (valid handle checks, buffer sizes, lifetime management)

At the end, provide a summary: total unsafe blocks found, how many have SAFETY comments, how many are flagged for review.
