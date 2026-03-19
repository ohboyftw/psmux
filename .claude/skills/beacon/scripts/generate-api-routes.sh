#!/usr/bin/env bash
# generate-api-routes.sh — Generate docs/generated/api-routes.md from framework routes
# Run from project root: bash scripts/generate-api-routes.sh

set -euo pipefail

OUTPUT="docs/generated/api-routes.md"
TODAY=$(date +%Y-%m-%d)

mkdir -p docs/generated

echo "Detecting API route source..."

# ─── Next.js App Router ─────────────────────────────────────────
if [ -d "app/api" ] || [ -d "src/app/api" ]; then
    API_DIR=$([ -d "app/api" ] && echo "app/api" || echo "src/app/api")
    echo "Found Next.js App Router API: $API_DIR"

    cat > "$OUTPUT" << 'HEADER'
---
title: "API Routes"
status: active
owner: auto-generated
last_verified: "TODAY_PLACEHOLDER"
generated_from: "API_DIR_PLACEHOLDER"
generator: "scripts/generate-api-routes.sh"
tags: [api, routes, generated]
cross_links:
  - ARCHITECTURE.md
  - docs/SECURITY.md
---

# API Routes

> **Auto-generated from route files**
> Do NOT edit manually. Regenerate with: `bash scripts/generate-api-routes.sh`

## Endpoints

HEADER

    sed -i "s/TODAY_PLACEHOLDER/$TODAY/g" "$OUTPUT"
    sed -i "s|API_DIR_PLACEHOLDER|$API_DIR|g" "$OUTPUT"

    # Find all route files and extract paths
    find "$API_DIR" -name "route.ts" -o -name "route.js" | sort | while read -r file; do
        # Convert file path to API path
        ROUTE=$(echo "$file" | sed "s|$API_DIR||" | sed 's|/route\.\(ts\|js\)||' | sed 's|\[|{|g' | sed 's|\]|}|g')
        
        # Detect HTTP methods
        METHODS=$(grep -oP '(GET|POST|PUT|PATCH|DELETE|HEAD|OPTIONS)' "$file" 2>/dev/null | sort -u | tr '\n' ', ' | sed 's/,$//')
        
        echo "| \`$ROUTE\` | $METHODS | \`$file\` |" >> "$OUTPUT"
    done

    # Add table header before the rows
    sed -i '/## Endpoints/a\| Route | Methods | Source |\n|-------|---------|--------|' "$OUTPUT"

    echo "" >> "$OUTPUT"
    echo "---" >> "$OUTPUT"
    echo "*Generated: $TODAY from $API_DIR*" >> "$OUTPUT"

    echo "Generated $OUTPUT from Next.js routes"

# ─── Express ─────────────────────────────────────────────────────
elif find . -name "*.ts" -o -name "*.js" | xargs grep -l "Router\(\)\|express()" 2>/dev/null | head -1 | grep -q .; then
    echo "Found Express app"

    cat > "$OUTPUT" << EOF
---
title: "API Routes"
status: active
owner: auto-generated
last_verified: "$TODAY"
generated_from: "Express route files"
generator: "scripts/generate-api-routes.sh"
tags: [api, routes, generated]
cross_links:
  - ARCHITECTURE.md
  - docs/SECURITY.md
---

# API Routes

> **Auto-generated from Express route files**
> Do NOT edit manually. Regenerate with: \`bash scripts/generate-api-routes.sh\`

## Route Files

$(find . -name "*.ts" -o -name "*.js" | xargs grep -l "router\.\(get\|post\|put\|delete\|patch\)" 2>/dev/null | sort | while read -r file; do
    echo "### \`$file\`"
    echo ""
    grep -nP "router\.(get|post|put|delete|patch)\(" "$file" 2>/dev/null | while read -r line; do
        echo "- $line"
    done
    echo ""
done)

---
*Generated: $TODAY*
EOF

    echo "Generated $OUTPUT from Express routes"

# ─── FastAPI ─────────────────────────────────────────────────────
elif find . -name "*.py" | xargs grep -l "@app\.\(get\|post\|put\|delete\)\|@router\.\(get\|post\|put\|delete\)" 2>/dev/null | head -1 | grep -q .; then
    echo "Found FastAPI app"

    cat > "$OUTPUT" << EOF
---
title: "API Routes"
status: active
owner: auto-generated
last_verified: "$TODAY"
generated_from: "FastAPI route files"
generator: "scripts/generate-api-routes.sh"
tags: [api, routes, generated]
cross_links:
  - ARCHITECTURE.md
  - docs/SECURITY.md
---

# API Routes

> **Auto-generated from FastAPI route files**
> Do NOT edit manually. Regenerate with: \`bash scripts/generate-api-routes.sh\`

## Route Files

$(find . -name "*.py" | xargs grep -l "@app\.\(get\|post\|put\|delete\)\|@router\.\(get\|post\|put\|delete\)" 2>/dev/null | sort | while read -r file; do
    echo "### \`$file\`"
    echo ""
    grep -nP "@(app|router)\.(get|post|put|delete|patch)" "$file" 2>/dev/null | while read -r line; do
        echo "- $line"
    done
    echo ""
done)

---
*Generated: $TODAY*
EOF

    echo "Generated $OUTPUT from FastAPI routes"

else
    echo "No supported API framework detected."
    echo "Supported: Next.js App Router, Express, FastAPI"
    exit 1
fi

echo "Done. Output: $OUTPUT"
