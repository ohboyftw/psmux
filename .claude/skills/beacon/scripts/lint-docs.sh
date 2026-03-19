#!/usr/bin/env bash
# lint-docs.sh — Knowledge base linter for the Serena pattern
# Run from project root: bash scripts/lint-docs.sh
# Exit codes: 0 = all pass, 1 = warnings only, 2 = errors found

set -euo pipefail

DOCS_DIR="docs"
AGENTS_FILE="AGENTS.md"
ARCH_FILE="ARCHITECTURE.md"
STALENESS_DAYS=30
PLAN_STALE_DAYS=14
AGENTS_MAX_LINES=100
AGENTS_WARN_LINES=80

ERRORS=0
WARNINGS=0

RED='\033[0;31m'
YELLOW='\033[1;33m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

error() { echo -e "${RED}ERROR:${NC} $1"; ((ERRORS++)); }
warn()  { echo -e "${YELLOW}WARN:${NC} $1"; ((WARNINGS++)); }
info()  { echo -e "${BLUE}INFO:${NC} $1"; }
pass()  { echo -e "${GREEN}PASS:${NC} $1"; }

echo "═══════════════════════════════════════════════"
echo "  Serena Knowledge Base Linter"
echo "═══════════════════════════════════════════════"
echo ""

# ─── Check 1: Required files exist ───────────────────────────────
info "Checking required files..."

if [ ! -f "$AGENTS_FILE" ]; then
    error "AGENTS.md not found at project root"
else
    pass "AGENTS.md exists"
fi

if [ ! -f "$ARCH_FILE" ]; then
    error "ARCHITECTURE.md not found at project root"
else
    pass "ARCHITECTURE.md exists"
fi

if [ ! -d "$DOCS_DIR" ]; then
    error "docs/ directory not found"
    echo ""
    echo "Result: $ERRORS errors, $WARNINGS warnings"
    exit 2
fi

# ─── Check 2: Required directories exist ─────────────────────────
info "Checking directory structure..."

REQUIRED_DIRS=(
    "docs/design-docs"
    "docs/exec-plans"
    "docs/exec-plans/active"
    "docs/exec-plans/completed"
    "docs/generated"
    "docs/product-specs"
    "docs/references"
)

for dir in "${REQUIRED_DIRS[@]}"; do
    if [ ! -d "$dir" ]; then
        error "Missing required directory: $dir"
    fi
done

# ─── Check 3: AGENTS.md line count ──────────────────────────────
if [ -f "$AGENTS_FILE" ]; then
    info "Checking AGENTS.md size..."
    LINE_COUNT=$(wc -l < "$AGENTS_FILE")
    if [ "$LINE_COUNT" -gt "$AGENTS_MAX_LINES" ]; then
        error "AGENTS.md is $LINE_COUNT lines (max: $AGENTS_MAX_LINES). It's a manual, not a map."
    elif [ "$LINE_COUNT" -gt "$AGENTS_WARN_LINES" ]; then
        warn "AGENTS.md is $LINE_COUNT lines (warn at: $AGENTS_WARN_LINES). Consider trimming."
    else
        pass "AGENTS.md is $LINE_COUNT lines (limit: $AGENTS_MAX_LINES)"
    fi
fi

# ─── Check 4: Frontmatter on all docs ───────────────────────────
info "Checking frontmatter..."

REQUIRED_FIELDS=("title" "status" "owner" "last_verified")

find "$DOCS_DIR" -name "*.md" -not -name "_template.md" | while read -r file; do
    # Check if file starts with ---
    if ! head -1 "$file" | grep -q "^---$"; then
        error "$file: Missing YAML frontmatter"
        continue
    fi

    # Extract frontmatter (between first two --- lines)
    FRONTMATTER=$(sed -n '1,/^---$/p' "$file" | tail -n +2 | head -n -1)

    if [ -z "$FRONTMATTER" ]; then
        error "$file: Empty frontmatter"
        continue
    fi

    for field in "${REQUIRED_FIELDS[@]}"; do
        if ! echo "$FRONTMATTER" | grep -q "^${field}:"; then
            error "$file: Missing frontmatter field '${field}'"
        fi
    done
done

# ─── Check 5: Freshness check ───────────────────────────────────
info "Checking doc freshness (${STALENESS_DAYS}-day threshold)..."

TODAY=$(date +%s)

find "$DOCS_DIR" -name "*.md" -not -path "*/generated/*" -not -name "_template.md" | while read -r file; do
    # Extract last_verified from frontmatter
    VERIFIED=$(grep "^last_verified:" "$file" 2>/dev/null | head -1 | sed 's/last_verified: *"\{0,1\}\([0-9-]*\)"\{0,1\}/\1/')

    if [ -z "$VERIFIED" ]; then
        continue  # Already caught by frontmatter check
    fi

    # Parse date (handles YYYY-MM-DD format)
    if VERIFIED_TS=$(date -d "$VERIFIED" +%s 2>/dev/null) || VERIFIED_TS=$(date -j -f "%Y-%m-%d" "$VERIFIED" +%s 2>/dev/null); then
        AGE_DAYS=$(( (TODAY - VERIFIED_TS) / 86400 ))
        if [ "$AGE_DAYS" -gt "$STALENESS_DAYS" ]; then
            warn "$file: Stale (last verified $VERIFIED, ${AGE_DAYS} days ago)"
        fi
    fi
done

# ─── Check 6: Cross-link validation ─────────────────────────────
info "Checking cross-links..."

find "$DOCS_DIR" -name "*.md" | while read -r file; do
    # Find markdown links to local .md files
    grep -oP '\[.*?\]\((?!http)(.*?\.md)\)' "$file" 2>/dev/null | \
    grep -oP '\((?!http)(.*?\.md)\)' | tr -d '()' | while read -r link; do
        # Resolve relative to the file's directory
        DIR=$(dirname "$file")
        TARGET=$(realpath --relative-to=. "$DIR/$link" 2>/dev/null || echo "$DIR/$link")

        if [ ! -f "$TARGET" ]; then
            error "$file: Broken cross-link to '$link' (resolved: $TARGET)"
        fi
    done
done

# ─── Check 7: Index files in directories ────────────────────────
info "Checking for index files..."

for dir in docs/design-docs docs/product-specs; do
    if [ -d "$dir" ] && [ ! -f "$dir/index.md" ]; then
        warn "$dir: Missing index.md"
    fi
done

# ─── Check 8: Active plan hygiene ───────────────────────────────
info "Checking active plan hygiene..."

if [ -d "docs/exec-plans/active" ]; then
    find "docs/exec-plans/active" -name "*.md" | while read -r plan; do
        # Check file modification time
        if [ "$(uname)" = "Darwin" ]; then
            MOD_TS=$(stat -f %m "$plan")
        else
            MOD_TS=$(stat -c %Y "$plan")
        fi
        AGE_DAYS=$(( (TODAY - MOD_TS) / 86400 ))

        if [ "$AGE_DAYS" -gt "$PLAN_STALE_DAYS" ]; then
            warn "$plan: Active plan with no updates in ${AGE_DAYS} days (threshold: ${PLAN_STALE_DAYS})"
        fi
    done
fi

# ─── Check 9: Orphaned docs ─────────────────────────────────────
info "Checking for orphaned docs..."

find "$DOCS_DIR" -name "*.md" -not -name "index.md" -not -name "_template.md" -not -path "*/generated/*" | while read -r file; do
    BASENAME=$(basename "$file")
    REL_PATH=$(echo "$file" | sed 's|^\./||')

    # Check if referenced in any index, AGENTS.md, or other doc
    REFERENCED=false
    if grep -rl "$BASENAME\|$REL_PATH" "$AGENTS_FILE" "$DOCS_DIR" 2>/dev/null | grep -v "$file" > /dev/null 2>&1; then
        REFERENCED=true
    fi

    if [ "$REFERENCED" = false ]; then
        warn "$file: Orphaned (not referenced by any index or AGENTS.md)"
    fi
done

# ─── Check 10: Tech debt tracker exists ─────────────────────────
if [ ! -f "docs/exec-plans/tech-debt-tracker.md" ]; then
    warn "Missing docs/exec-plans/tech-debt-tracker.md"
fi

# ─── Summary ─────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════════"
if [ "$ERRORS" -gt 0 ]; then
    echo -e "  ${RED}FAILED:${NC} $ERRORS errors, $WARNINGS warnings"
    exit 2
elif [ "$WARNINGS" -gt 0 ]; then
    echo -e "  ${YELLOW}PASSED WITH WARNINGS:${NC} $WARNINGS warnings"
    exit 1
else
    echo -e "  ${GREEN}ALL CHECKS PASSED${NC}"
    exit 0
fi
