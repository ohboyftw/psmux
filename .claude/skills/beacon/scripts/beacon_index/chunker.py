"""
Markdown-aware document chunker for the Beacon knowledge base.

Chunks documents by semantic boundaries (headings, frontmatter, code blocks)
rather than naive character splits. Each chunk retains its document path,
heading hierarchy, and frontmatter metadata for precise retrieval.
"""

from __future__ import annotations

import re
import yaml
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional


@dataclass
class Chunk:
    """A single retrievable unit from the knowledge base."""

    doc_path: str  # Relative path from project root
    heading_path: list[str]  # e.g. ["Architecture Overview", "Domain Map"]
    content: str  # The actual text content
    chunk_type: str  # "prose", "code", "frontmatter", "table", "list"
    start_line: int  # Line number in source file
    end_line: int
    frontmatter: dict = field(default_factory=dict)  # Parsed YAML frontmatter
    tokens_approx: int = 0  # Rough token count (words * 1.3)

    @property
    def chunk_id(self) -> str:
        """Stable identifier for this chunk."""
        heading = " > ".join(self.heading_path) if self.heading_path else "root"
        return f"{self.doc_path}::{heading}::L{self.start_line}"

    @property
    def context_header(self) -> str:
        """Human-readable context prefix for retrieval results."""
        parts = [f"[{self.doc_path}]"]
        if self.heading_path:
            parts.append(" > ".join(self.heading_path))
        parts.append(f"(lines {self.start_line}-{self.end_line})")
        return " ".join(parts)

    def to_dict(self) -> dict:
        return {
            "chunk_id": self.chunk_id,
            "doc_path": self.doc_path,
            "heading_path": self.heading_path,
            "content": self.content,
            "chunk_type": self.chunk_type,
            "start_line": self.start_line,
            "end_line": self.end_line,
            "frontmatter": self.frontmatter,
            "tokens_approx": self.tokens_approx,
        }


# ─── Chunking Configuration ─────────────────────────────────────

MAX_CHUNK_TOKENS = 512  # Target max size per chunk
MIN_CHUNK_TOKENS = 30  # Don't create tiny chunks, merge with previous
OVERLAP_LINES = 2  # Lines of overlap between consecutive prose chunks


def estimate_tokens(text: str) -> int:
    """Rough token estimate: word count * 1.3 (covers subword tokenization)."""
    return int(len(text.split()) * 1.3)


# ─── Frontmatter Extraction ─────────────────────────────────────


def extract_frontmatter(text: str) -> tuple[dict, str]:
    """Extract YAML frontmatter and return (metadata, remaining_text)."""
    if not text.startswith("---"):
        return {}, text

    parts = text.split("---", 2)
    if len(parts) < 3:
        return {}, text

    try:
        fm = yaml.safe_load(parts[1])
        if not isinstance(fm, dict):
            return {}, text
        return fm, parts[2].lstrip("\n")
    except yaml.YAMLError:
        return {}, text


# ─── Line Classification ────────────────────────────────────────

HEADING_RE = re.compile(r"^(#{1,6})\s+(.+)$")
CODE_FENCE_RE = re.compile(r"^```")
TABLE_ROW_RE = re.compile(r"^\|.*\|$")
LIST_ITEM_RE = re.compile(r"^[\s]*[-*+]\s|^[\s]*\d+\.\s")


def classify_line(line: str) -> str:
    """Classify a single line by type."""
    stripped = line.strip()
    if HEADING_RE.match(stripped):
        return "heading"
    if CODE_FENCE_RE.match(stripped):
        return "code_fence"
    if TABLE_ROW_RE.match(stripped):
        return "table"
    if LIST_ITEM_RE.match(stripped):
        return "list"
    if stripped == "":
        return "blank"
    return "prose"


# ─── Core Chunking Engine ───────────────────────────────────────


def chunk_document(doc_path: str, text: str) -> list[Chunk]:
    """
    Chunk a markdown document into semantically meaningful pieces.

    Strategy:
    1. Extract frontmatter as its own chunk
    2. Split on heading boundaries (## and above create hard breaks)
    3. Within sections, split on block boundaries (code, tables, lists)
    4. Prose blocks are split at paragraph boundaries if they exceed MAX_CHUNK_TOKENS
    5. Tiny chunks are merged with their predecessor
    """
    frontmatter, body = extract_frontmatter(text)
    lines = body.split("\n")
    chunks: list[Chunk] = []

    # Frontmatter chunk (always include — it's the doc's identity)
    if frontmatter:
        fm_text = yaml.dump(frontmatter, default_flow_style=False).strip()
        chunks.append(
            Chunk(
                doc_path=doc_path,
                heading_path=[],
                content=fm_text,
                chunk_type="frontmatter",
                start_line=1,
                end_line=1,
                frontmatter=frontmatter,
                tokens_approx=estimate_tokens(fm_text),
            )
        )

    # State machine for chunking
    current_headings: list[str] = []
    current_lines: list[str] = []
    current_type = "prose"
    current_start = 1
    in_code_block = False
    # Track the line offset from frontmatter
    fm_lines = text.split("---", 2)
    line_offset = len(fm_lines[1].split("\n")) + 2 if len(fm_lines) >= 3 and text.startswith("---") else 0

    def flush_chunk():
        nonlocal current_lines, current_type, current_start
        if not current_lines:
            return
        content = "\n".join(current_lines).strip()
        if not content:
            current_lines = []
            return

        tokens = estimate_tokens(content)

        # If chunk is too small and we have a previous non-frontmatter chunk, merge
        if tokens < MIN_CHUNK_TOKENS and chunks and chunks[-1].chunk_type != "frontmatter":
            prev = chunks[-1]
            prev.content += "\n\n" + content
            prev.end_line = current_start + len(current_lines) - 1
            prev.tokens_approx = estimate_tokens(prev.content)
        else:
            # If chunk is too large, split at paragraph boundaries
            if tokens > MAX_CHUNK_TOKENS and current_type == "prose":
                sub_chunks = split_large_prose(
                    content,
                    doc_path,
                    list(current_headings),
                    frontmatter,
                    current_start + line_offset,
                )
                chunks.extend(sub_chunks)
            else:
                chunks.append(
                    Chunk(
                        doc_path=doc_path,
                        heading_path=list(current_headings),
                        content=content,
                        chunk_type=current_type,
                        start_line=current_start + line_offset,
                        end_line=current_start + len(current_lines) - 1 + line_offset,
                        frontmatter=frontmatter,
                        tokens_approx=tokens,
                    )
                )
        current_lines = []

    for i, line in enumerate(lines, start=1):
        line_type = classify_line(line)

        # Handle code fences
        if line_type == "code_fence":
            if in_code_block:
                # End of code block
                current_lines.append(line)
                in_code_block = False
                flush_chunk()
                current_start = i + 1
                current_type = "prose"
                continue
            else:
                # Start of code block — flush what we have, start code chunk
                flush_chunk()
                in_code_block = True
                current_type = "code"
                current_start = i
                current_lines = [line]
                continue

        if in_code_block:
            current_lines.append(line)
            continue

        # Headings create hard chunk boundaries
        if line_type == "heading":
            flush_chunk()
            match = HEADING_RE.match(line.strip())
            level = len(match.group(1))
            title = match.group(2).strip()

            # Update heading hierarchy
            # Level 1 = index 0, level 2 = index 1, etc.
            while len(current_headings) >= level:
                current_headings.pop()
            current_headings.append(title)

            current_start = i
            current_type = "prose"
            current_lines = [line]
            continue

        # Tables: group consecutive table rows
        if line_type == "table":
            if current_type != "table":
                flush_chunk()
                current_type = "table"
                current_start = i
            current_lines.append(line)
            continue
        elif current_type == "table" and line_type != "table":
            flush_chunk()
            current_type = "prose"
            current_start = i

        # Lists: group consecutive list items
        if line_type == "list":
            if current_type != "list":
                flush_chunk()
                current_type = "list"
                current_start = i
            current_lines.append(line)
            continue
        elif current_type == "list" and line_type not in ("list", "blank"):
            flush_chunk()
            current_type = "prose"
            current_start = i

        # Default: accumulate prose
        if not current_lines:
            current_start = i
        current_lines.append(line)

    # Flush remaining
    flush_chunk()

    return chunks


def split_large_prose(
    content: str,
    doc_path: str,
    headings: list[str],
    frontmatter: dict,
    start_line: int,
) -> list[Chunk]:
    """Split large prose blocks at paragraph boundaries."""
    paragraphs = re.split(r"\n\n+", content)
    chunks = []
    current_paras = []
    current_tokens = 0
    current_line = start_line

    for para in paragraphs:
        para_tokens = estimate_tokens(para)
        para_lines = len(para.split("\n"))

        if current_tokens + para_tokens > MAX_CHUNK_TOKENS and current_paras:
            # Flush current accumulation
            text = "\n\n".join(current_paras)
            chunks.append(
                Chunk(
                    doc_path=doc_path,
                    heading_path=list(headings),
                    content=text,
                    chunk_type="prose",
                    start_line=current_line,
                    end_line=current_line + sum(len(p.split("\n")) for p in current_paras) - 1,
                    frontmatter=frontmatter,
                    tokens_approx=estimate_tokens(text),
                )
            )
            current_line += sum(len(p.split("\n")) for p in current_paras) + len(current_paras)
            current_paras = []
            current_tokens = 0

        current_paras.append(para)
        current_tokens += para_tokens

    # Flush remaining
    if current_paras:
        text = "\n\n".join(current_paras)
        chunks.append(
            Chunk(
                doc_path=doc_path,
                heading_path=list(headings),
                content=text,
                chunk_type="prose",
                start_line=current_line,
                end_line=current_line + sum(len(p.split("\n")) for p in current_paras) - 1,
                frontmatter=frontmatter,
                tokens_approx=estimate_tokens(text),
            )
        )

    return chunks


# ─── Bulk Indexing ───────────────────────────────────────────────


def index_knowledge_base(root: str | Path, glob_pattern: str = "**/*.md") -> list[Chunk]:
    """
    Walk the knowledge base and chunk all markdown files.

    Args:
        root: Project root directory
        glob_pattern: File pattern to index (default: all .md files)

    Returns:
        List of all chunks across all documents
    """
    root = Path(root)
    all_chunks = []

    # Index AGENTS.md and ARCHITECTURE.md at root
    for root_doc in ["AGENTS.md", "ARCHITECTURE.md"]:
        path = root / root_doc
        if path.exists():
            text = path.read_text(encoding="utf-8", errors="replace")
            chunks = chunk_document(root_doc, text)
            all_chunks.extend(chunks)

    # Index docs/ directory
    docs_dir = root / "docs"
    if docs_dir.exists():
        for md_file in sorted(docs_dir.glob(glob_pattern)):
            rel_path = str(md_file.relative_to(root))
            text = md_file.read_text(encoding="utf-8", errors="replace")
            chunks = chunk_document(rel_path, text)
            all_chunks.extend(chunks)

    return all_chunks
