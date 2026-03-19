#!/usr/bin/env bash
# generate-db-schema.sh — Generate docs/generated/db-schema.md from ORM/SQL
# Run from project root: bash scripts/generate-db-schema.sh

set -euo pipefail

OUTPUT="docs/generated/db-schema.md"
TODAY=$(date +%Y-%m-%d)

mkdir -p docs/generated

echo "Detecting database schema source..."

# ─── Prisma ──────────────────────────────────────────────────────
if [ -f "prisma/schema.prisma" ]; then
    SOURCE="prisma/schema.prisma"
    echo "Found Prisma schema: $SOURCE"

    cat > "$OUTPUT" << EOF
---
title: "Database Schema"
status: active
owner: auto-generated
last_verified: "$TODAY"
generated_from: "$SOURCE"
generator: "scripts/generate-db-schema.sh"
tags: [database, schema, generated]
cross_links:
  - ARCHITECTURE.md
---

# Database Schema

> **Auto-generated from \`$SOURCE\`**
> Do NOT edit manually. Regenerate with: \`bash scripts/generate-db-schema.sh\`

## Models

\`\`\`prisma
$(cat "$SOURCE")
\`\`\`

## Quick Reference

| Model | Key Fields | Relations |
|-------|-----------|-----------|
$(grep "^model " "$SOURCE" | sed 's/model \(.*\) {/| \1 | — | — |/')

---
*Generated: $TODAY from $SOURCE*
EOF

    echo "Generated $OUTPUT from Prisma schema"

# ─── SQLAlchemy ──────────────────────────────────────────────────
elif find . -name "models.py" -path "*/models.py" | head -1 | grep -q .; then
    SOURCE=$(find . -name "models.py" -path "*/models.py" | head -1)
    echo "Found SQLAlchemy models: $SOURCE"

    cat > "$OUTPUT" << EOF
---
title: "Database Schema"
status: active
owner: auto-generated
last_verified: "$TODAY"
generated_from: "$SOURCE"
generator: "scripts/generate-db-schema.sh"
tags: [database, schema, generated]
cross_links:
  - ARCHITECTURE.md
---

# Database Schema

> **Auto-generated from \`$SOURCE\`**
> Do NOT edit manually. Regenerate with: \`bash scripts/generate-db-schema.sh\`

## Models

\`\`\`python
$(cat "$SOURCE")
\`\`\`

---
*Generated: $TODAY from $SOURCE*
EOF

    echo "Generated $OUTPUT from SQLAlchemy models"

# ─── Drizzle ─────────────────────────────────────────────────────
elif find . -name "schema.ts" -path "*/drizzle/*" -o -name "schema.ts" -path "*/db/*" 2>/dev/null | head -1 | grep -q .; then
    SOURCE=$(find . -name "schema.ts" \( -path "*/drizzle/*" -o -path "*/db/*" \) | head -1)
    echo "Found Drizzle schema: $SOURCE"

    cat > "$OUTPUT" << EOF
---
title: "Database Schema"
status: active
owner: auto-generated
last_verified: "$TODAY"
generated_from: "$SOURCE"
generator: "scripts/generate-db-schema.sh"
tags: [database, schema, generated]
cross_links:
  - ARCHITECTURE.md
---

# Database Schema

> **Auto-generated from \`$SOURCE\`**
> Do NOT edit manually. Regenerate with: \`bash scripts/generate-db-schema.sh\`

## Schema

\`\`\`typescript
$(cat "$SOURCE")
\`\`\`

---
*Generated: $TODAY from $SOURCE*
EOF

    echo "Generated $OUTPUT from Drizzle schema"

# ─── No ORM found ───────────────────────────────────────────────
else
    echo "No supported ORM schema found."
    echo "Supported: Prisma (prisma/schema.prisma), SQLAlchemy (models.py), Drizzle (drizzle/schema.ts)"
    echo "Create the schema file first, then re-run this script."
    exit 1
fi

echo "Done. Output: $OUTPUT"
