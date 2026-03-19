"""
Doc Gardener: Python-based knowledge base maintenance engine.

Replaces the bash linter with a more capable system that can:
1. Run all structural/freshness checks (like lint-docs.sh)
2. Detect semantic drift (doc content doesn't match code)
3. Find near-duplicate content across docs
4. Suggest missing cross-links based on content similarity
5. Generate a comprehensive health report

Usage:
    python -m beacon_index.gardener /path/to/project
    python -m beacon_index.gardener /path/to/project --fix  # Auto-fix safe issues
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import yaml
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from pathlib import Path
from typing import Optional

from .chunker import Chunk, index_knowledge_base, extract_frontmatter
from .bm25 import BM25Index, tokenize


# ─── Configuration ───────────────────────────────────────────────

STALENESS_DAYS = 30
PLAN_STALE_DAYS = 14
AGENTS_MAX_LINES = 100
AGENTS_WARN_LINES = 80
REQUIRED_FRONTMATTER = ["title", "status", "owner", "last_verified"]
VALID_STATUSES = {"draft", "active", "stale", "archived"}
DUPLICATE_SIMILARITY_THRESHOLD = 0.7  # Jaccard similarity for near-duplicate detection


# ─── Issue Types ─────────────────────────────────────────────────


@dataclass
class Issue:
    """A single problem found in the knowledge base."""

    severity: str  # "error", "warning", "info"
    category: str  # "structure", "freshness", "links", "content", "duplicates"
    file: str
    message: str
    auto_fixable: bool = False
    fix_action: Optional[str] = None  # Description of auto-fix


@dataclass
class GardenReport:
    """Full health report for the knowledge base."""

    timestamp: str
    project_root: str
    issues: list[Issue] = field(default_factory=list)
    stats: dict = field(default_factory=dict)
    auto_fixed: list[str] = field(default_factory=list)

    @property
    def errors(self) -> list[Issue]:
        return [i for i in self.issues if i.severity == "error"]

    @property
    def warnings(self) -> list[Issue]:
        return [i for i in self.issues if i.severity == "warning"]

    @property
    def is_healthy(self) -> bool:
        return len(self.errors) == 0

    def to_markdown(self) -> str:
        """Generate a markdown health report."""
        lines = [
            f"# Doc Garden Report — {self.timestamp}",
            "",
            "## Summary",
            f"- **{len(self.errors)} errors** (must fix)",
            f"- **{len(self.warnings)} warnings** (should review)",
            f"- **{len(self.auto_fixed)} auto-fixed**",
            f"- **{self.stats.get('total_docs', 0)} documents** indexed",
            f"- **{self.stats.get('total_chunks', 0)} chunks** in knowledge base",
            "",
        ]

        if self.errors:
            lines.append("## Errors\n")
            for i, issue in enumerate(self.errors, 1):
                lines.append(f"{i}. `{issue.file}`: {issue.message}")
            lines.append("")

        if self.warnings:
            lines.append("## Warnings\n")
            for i, issue in enumerate(self.warnings, 1):
                lines.append(f"{i}. `{issue.file}`: {issue.message}")
            lines.append("")

        if self.auto_fixed:
            lines.append("## Auto-Fixed\n")
            for fix in self.auto_fixed:
                lines.append(f"- {fix}")
            lines.append("")

        # Category breakdown
        cats = defaultdict(int)
        for issue in self.issues:
            cats[issue.category] += 1
        if cats:
            lines.append("## Issues by Category\n")
            lines.append("| Category | Count |")
            lines.append("|----------|-------|")
            for cat, count in sorted(cats.items()):
                lines.append(f"| {cat} | {count} |")
            lines.append("")

        return "\n".join(lines)

    def to_json(self) -> str:
        return json.dumps(
            {
                "timestamp": self.timestamp,
                "project_root": self.project_root,
                "summary": {
                    "errors": len(self.errors),
                    "warnings": len(self.warnings),
                    "auto_fixed": len(self.auto_fixed),
                },
                "issues": [
                    {
                        "severity": i.severity,
                        "category": i.category,
                        "file": i.file,
                        "message": i.message,
                        "auto_fixable": i.auto_fixable,
                    }
                    for i in self.issues
                ],
                "stats": self.stats,
            },
            indent=2,
        )


# ─── Gardener Engine ─────────────────────────────────────────────


class DocGardener:
    """
    Knowledge base health checker and maintenance engine.

    Runs structural checks, freshness scans, cross-link validation,
    near-duplicate detection, and optionally auto-fixes safe issues.
    """

    def __init__(self, project_root: str | Path):
        self.root = Path(project_root).resolve()
        self.docs_dir = self.root / "docs"
        self.issues: list[Issue] = []
        self.auto_fixed: list[str] = []
        self.chunks: list[Chunk] = []
        self.bm25: Optional[BM25Index] = None

    def _add(self, severity: str, category: str, file: str, message: str, **kwargs):
        self.issues.append(
            Issue(severity=severity, category=category, file=file, message=message, **kwargs)
        )

    # ─── Check 1: Structure ──────────────────────────────────────

    def check_structure(self):
        """Verify required files and directories exist."""

        # Required root files
        for f in ["AGENTS.md", "ARCHITECTURE.md"]:
            if not (self.root / f).exists():
                self._add("error", "structure", f, f"{f} not found at project root")

        # Required directories
        required_dirs = [
            "docs/design-docs",
            "docs/exec-plans",
            "docs/exec-plans/active",
            "docs/exec-plans/completed",
            "docs/generated",
            "docs/product-specs",
            "docs/references",
        ]
        for d in required_dirs:
            if not (self.root / d).is_dir():
                self._add("error", "structure", d, f"Missing required directory: {d}")

        # Required template
        template = self.root / "docs/exec-plans/_template.md"
        if not template.exists():
            self._add("warning", "structure", str(template), "Missing execution plan template")

        # Tech debt tracker
        tracker = self.root / "docs/exec-plans/tech-debt-tracker.md"
        if not tracker.exists():
            self._add("warning", "structure", str(tracker), "Missing tech-debt-tracker.md")

    # ─── Check 2: AGENTS.md Size ─────────────────────────────────

    def check_agents_size(self):
        """Verify AGENTS.md stays within size limits."""
        agents = self.root / "AGENTS.md"
        if not agents.exists():
            return

        lines = agents.read_text().split("\n")
        n = len(lines)

        if n > AGENTS_MAX_LINES:
            self._add(
                "error", "structure", "AGENTS.md",
                f"AGENTS.md is {n} lines (max: {AGENTS_MAX_LINES}). It's a manual, not a map."
            )
        elif n > AGENTS_WARN_LINES:
            self._add(
                "warning", "structure", "AGENTS.md",
                f"AGENTS.md is {n} lines (warn: {AGENTS_WARN_LINES}). Consider trimming."
            )

    # ─── Check 3: Frontmatter ────────────────────────────────────

    def check_frontmatter(self):
        """Validate YAML frontmatter on all docs."""
        if not self.docs_dir.exists():
            return

        for md in sorted(self.docs_dir.rglob("*.md")):
            if md.name == "_template.md":
                continue

            rel = str(md.relative_to(self.root))
            text = md.read_text(encoding="utf-8", errors="replace")

            if not text.startswith("---"):
                self._add("error", "frontmatter", rel, "Missing YAML frontmatter")
                continue

            fm, _ = extract_frontmatter(text)
            if not fm:
                self._add("error", "frontmatter", rel, "Empty or invalid frontmatter")
                continue

            for field_name in REQUIRED_FRONTMATTER:
                if field_name not in fm:
                    self._add("error", "frontmatter", rel, f"Missing field: {field_name}")

            # Validate status value
            status = fm.get("status", "")
            if status and str(status) not in VALID_STATUSES:
                self._add(
                    "warning", "frontmatter", rel,
                    f"Invalid status '{status}' (expected: {', '.join(VALID_STATUSES)})"
                )

    # ─── Check 4: Freshness ──────────────────────────────────────

    def check_freshness(self, auto_fix: bool = False):
        """Detect stale documents based on last_verified date."""
        if not self.docs_dir.exists():
            return

        today = datetime.now()

        for md in sorted(self.docs_dir.rglob("*.md")):
            if md.name == "_template.md" or "generated" in str(md):
                continue

            rel = str(md.relative_to(self.root))
            text = md.read_text(encoding="utf-8", errors="replace")
            fm, _ = extract_frontmatter(text)

            if not fm or "last_verified" not in fm:
                continue

            try:
                verified_str = str(fm["last_verified"])
                verified = datetime.fromisoformat(verified_str)
                age = (today - verified).days

                if age > STALENESS_DAYS:
                    status = fm.get("status", "")
                    if status == "active":
                        self._add(
                            "warning", "freshness", rel,
                            f"Stale: last verified {verified_str} ({age} days ago)",
                            auto_fixable=True,
                            fix_action=f"Set status to 'stale'"
                        )

                        if auto_fix:
                            self._auto_set_stale(md, text)

            except (ValueError, TypeError):
                self._add("warning", "freshness", rel, f"Invalid date in last_verified: {fm['last_verified']}")

        # Check active plans for staleness
        active_plans = self.root / "docs/exec-plans/active"
        if active_plans.is_dir():
            for plan in active_plans.glob("*.md"):
                rel = str(plan.relative_to(self.root))
                mod_time = datetime.fromtimestamp(plan.stat().st_mtime)
                age = (today - mod_time).days

                if age > PLAN_STALE_DAYS:
                    self._add(
                        "warning", "freshness", rel,
                        f"Active plan with no updates in {age} days (threshold: {PLAN_STALE_DAYS})"
                    )

    def _auto_set_stale(self, path: Path, text: str):
        """Auto-fix: set status to stale."""
        updated = re.sub(
            r'^(status:\s*)active',
            r'\1stale',
            text,
            flags=re.MULTILINE,
        )
        if updated != text:
            path.write_text(updated)
            rel = str(path.relative_to(self.root))
            self.auto_fixed.append(f"{rel}: Set status from 'active' to 'stale'")

    # ─── Check 5: Cross-Links ────────────────────────────────────

    def check_cross_links(self):
        """Validate that all markdown cross-links resolve."""
        all_md = list(self.root.glob("*.md")) + list(self.docs_dir.rglob("*.md")) if self.docs_dir.exists() else list(self.root.glob("*.md"))

        for md in all_md:
            rel = str(md.relative_to(self.root))
            text = md.read_text(encoding="utf-8", errors="replace")

            # Find local .md links (not http)
            links = re.findall(r'\[.*?\]\((?!http)(.*?\.md)\)', text)

            for link in links:
                # Resolve relative to file's directory
                target = (md.parent / link).resolve()
                if not target.exists():
                    self._add("error", "links", rel, f"Broken link to '{link}'")

        # Check frontmatter cross_links
        if self.docs_dir.exists():
            for md in self.docs_dir.rglob("*.md"):
                rel = str(md.relative_to(self.root))
                text = md.read_text(encoding="utf-8", errors="replace")
                fm, _ = extract_frontmatter(text)

                if fm and "cross_links" in fm and isinstance(fm["cross_links"], list):
                    for link in fm["cross_links"]:
                        target = (self.root / link).resolve()
                        if not target.exists():
                            self._add("error", "links", rel, f"Broken cross_link in frontmatter: '{link}'")

    # ─── Check 6: Orphaned Docs ──────────────────────────────────

    def check_orphans(self):
        """Find docs not referenced by any index or AGENTS.md."""
        if not self.docs_dir.exists():
            return

        # Build set of all .md file basenames and relative paths
        all_docs = set()
        for md in self.docs_dir.rglob("*.md"):
            if md.name in ("index.md", "_template.md") or "generated" in str(md):
                continue
            all_docs.add(str(md.relative_to(self.root)))

        # Build set of all referenced files
        referenced = set()

        # Check AGENTS.md
        agents = self.root / "AGENTS.md"
        if agents.exists():
            text = agents.read_text()
            for doc in all_docs:
                basename = Path(doc).name
                if basename in text or doc in text:
                    referenced.add(doc)

        # Check all index.md files
        for index in self.docs_dir.rglob("index.md"):
            text = index.read_text()
            for doc in all_docs:
                basename = Path(doc).name
                if basename in text or doc in text:
                    referenced.add(doc)

        # Check frontmatter cross_links in all docs
        for md in self.docs_dir.rglob("*.md"):
            text = md.read_text(encoding="utf-8", errors="replace")
            fm, _ = extract_frontmatter(text)
            if fm and "cross_links" in fm and isinstance(fm["cross_links"], list):
                for link in fm["cross_links"]:
                    referenced.add(link)

        orphans = all_docs - referenced
        for orphan in sorted(orphans):
            self._add("warning", "orphans", orphan, "Orphaned: not referenced by any index or AGENTS.md")

    # ─── Check 7: Index Files ────────────────────────────────────

    def check_indexes(self, auto_fix: bool = False):
        """Verify each docs/ subdirectory has an index.md."""
        if not self.docs_dir.exists():
            return

        for subdir in sorted(self.docs_dir.iterdir()):
            if not subdir.is_dir() or subdir.name in ("generated", "references"):
                continue

            index = subdir / "index.md"
            if not index.exists():
                rel = str(subdir.relative_to(self.root))
                self._add(
                    "warning", "structure", rel,
                    f"Missing index.md",
                    auto_fixable=True,
                    fix_action="Create placeholder index.md"
                )

                if auto_fix:
                    self._auto_create_index(subdir)

    def _auto_create_index(self, directory: Path):
        """Auto-fix: create a placeholder index.md."""
        index = directory / "index.md"
        name = directory.name.replace("-", " ").title()
        today = datetime.now().strftime("%Y-%m-%d")

        content = f"""---
title: "{name} Index"
status: draft
owner: "auto-generated"
last_verified: "{today}"
tags: [index]
cross_links:
  - AGENTS.md
---

# {name}

> This index was auto-generated. Please fill in descriptions.

## Documents

"""
        # List existing .md files
        for md in sorted(directory.glob("*.md")):
            if md.name != "index.md":
                content += f"- [{md.name}]({md.name}) — TODO: add description\n"

        index.write_text(content)
        rel = str(directory.relative_to(self.root))
        self.auto_fixed.append(f"{rel}/index.md: Created placeholder index")

    # ─── Check 8: Near-Duplicate Detection ───────────────────────

    def check_duplicates(self):
        """
        Find near-duplicate content across documents using token Jaccard similarity.

        This catches copy-pasted content that should be consolidated (link, don't duplicate).
        """
        if not self.chunks:
            self.chunks = index_knowledge_base(self.root)

        if not self.chunks:
            return

        # Only check prose chunks of meaningful size
        prose_chunks = [
            c for c in self.chunks
            if c.chunk_type == "prose" and c.tokens_approx > 50
        ]

        # Compare each pair using token Jaccard similarity
        seen_pairs = set()
        for i, a in enumerate(prose_chunks):
            tokens_a = set(tokenize(a.content))
            if len(tokens_a) < 10:
                continue

            for j, b in enumerate(prose_chunks[i + 1:], i + 1):
                # Skip same-document comparisons
                if a.doc_path == b.doc_path:
                    continue

                pair_key = (min(a.chunk_id, b.chunk_id), max(a.chunk_id, b.chunk_id))
                if pair_key in seen_pairs:
                    continue
                seen_pairs.add(pair_key)

                tokens_b = set(tokenize(b.content))
                if len(tokens_b) < 10:
                    continue

                # Jaccard similarity
                intersection = len(tokens_a & tokens_b)
                union = len(tokens_a | tokens_b)
                similarity = intersection / union if union > 0 else 0

                if similarity >= DUPLICATE_SIMILARITY_THRESHOLD:
                    self._add(
                        "warning", "duplicates", a.doc_path,
                        f"Near-duplicate content ({similarity:.0%} similar) with {b.doc_path} "
                        f"[{' > '.join(b.heading_path) if b.heading_path else 'root'}]. "
                        f"Consider consolidating (link, don't duplicate)."
                    )

    # ─── Check 9: Missing Cross-Link Suggestions ─────────────────

    def suggest_cross_links(self, top_k: int = 3):
        """
        Use BM25 to suggest missing cross-links between documents.

        For each document, check if highly related documents are linked.
        """
        if not self.chunks:
            self.chunks = index_knowledge_base(self.root)

        if len(self.chunks) < 5:
            return

        # Build BM25 index
        self.bm25 = BM25Index()
        self.bm25.add_chunks(self.chunks)

        # For each document, find most related docs
        doc_chunks: dict[str, list[Chunk]] = defaultdict(list)
        for chunk in self.chunks:
            doc_chunks[chunk.doc_path].append(chunk)

        for doc_path, chunks in doc_chunks.items():
            # Build a query from the document's key content
            doc_text = " ".join(c.content[:200] for c in chunks[:3])

            results = self.bm25.search(doc_text, top_k=top_k * 2)

            # Filter to different documents
            related_docs = []
            for r in results:
                if r.chunk.doc_path != doc_path and r.chunk.doc_path not in related_docs:
                    related_docs.append(r.chunk.doc_path)
                    if len(related_docs) >= top_k:
                        break

            # Check if these are already cross-linked
            fm = chunks[0].frontmatter if chunks else {}
            existing_links = set(fm.get("cross_links", []))

            # Also check inline links
            full_text = " ".join(c.content for c in chunks)
            for related in related_docs:
                basename = Path(related).name
                if (
                    related not in existing_links
                    and basename not in full_text
                    and related not in full_text
                ):
                    self._add(
                        "info", "links", doc_path,
                        f"Consider cross-linking to '{related}' (content similarity detected)"
                    )

    # ─── Run All Checks ──────────────────────────────────────────

    def run(self, auto_fix: bool = False, deep: bool = False) -> GardenReport:
        """
        Run all gardening checks.

        Args:
            auto_fix: Automatically fix safe issues (stale status, missing indexes)
            deep: Run expensive checks (duplicate detection, cross-link suggestions)
        """
        self.issues = []
        self.auto_fixed = []

        # Core checks (fast)
        self.check_structure()
        self.check_agents_size()
        self.check_frontmatter()
        self.check_freshness(auto_fix=auto_fix)
        self.check_cross_links()
        self.check_orphans()
        self.check_indexes(auto_fix=auto_fix)

        # Deep checks (slower, use BM25)
        if deep:
            self.check_duplicates()
            self.suggest_cross_links()

        # Build stats
        doc_count = 0
        if self.docs_dir.exists():
            doc_count = sum(1 for _ in self.docs_dir.rglob("*.md"))
        for f in ["AGENTS.md", "ARCHITECTURE.md"]:
            if (self.root / f).exists():
                doc_count += 1

        report = GardenReport(
            timestamp=datetime.now().strftime("%Y-%m-%d %H:%M"),
            project_root=str(self.root),
            issues=self.issues,
            stats={
                "total_docs": doc_count,
                "total_chunks": len(self.chunks) if self.chunks else "not indexed",
                "checks_run": "deep" if deep else "standard",
            },
            auto_fixed=self.auto_fixed,
        )

        return report


# ─── CLI Entry Point ─────────────────────────────────────────────


def main():
    parser = argparse.ArgumentParser(
        description="Beacon Doc Gardener — Knowledge base health checker"
    )
    parser.add_argument("project_root", help="Path to project root")
    parser.add_argument("--fix", action="store_true", help="Auto-fix safe issues")
    parser.add_argument("--deep", action="store_true", help="Run deep checks (duplicates, cross-link suggestions)")
    parser.add_argument("--json", action="store_true", help="Output as JSON instead of markdown")
    parser.add_argument("--output", "-o", help="Write report to file instead of stdout")

    args = parser.parse_args()

    gardener = DocGardener(args.project_root)
    report = gardener.run(auto_fix=args.fix, deep=args.deep)

    output = report.to_json() if args.json else report.to_markdown()

    if args.output:
        Path(args.output).write_text(output)
        print(f"Report written to {args.output}")
    else:
        print(output)

    # Exit code
    sys.exit(2 if report.errors else 1 if report.warnings else 0)


if __name__ == "__main__":
    main()
