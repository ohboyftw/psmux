"""
BM25 retriever for the Beacon knowledge base.

Zero external dependencies. Implements Okapi BM25 scoring with:
- Markdown-aware tokenization (strips formatting, preserves code tokens)
- Bigram indexing for phrase matching
- Frontmatter field boosting (title, tags get higher weight)
- Heading path boosting (matches in headings rank higher)
"""

from __future__ import annotations

import math
import re
import json
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from .chunker import Chunk

# ─── BM25 Parameters ────────────────────────────────────────────

K1 = 1.5  # Term frequency saturation
B = 0.75  # Length normalization (0 = no normalization, 1 = full)
TITLE_BOOST = 2.0  # Boost for matches in title/heading
TAG_BOOST = 1.5  # Boost for matches in frontmatter tags
BIGRAM_BOOST = 1.3  # Boost for bigram matches (phrase-like)


# ─── Tokenization ────────────────────────────────────────────────

# Strip markdown formatting but preserve meaningful tokens
MD_STRIP_RE = re.compile(
    r"[#*`\[\](){}|>!~_]"  # Markdown syntax chars
)
WORD_RE = re.compile(r"[a-zA-Z0-9][\w\-\.]*[a-zA-Z0-9]|[a-zA-Z0-9]")

# Common stop words (keep short for code-heavy docs)
STOP_WORDS = frozenset(
    "a an and are as at be by for from has have in is it its of on or that the "
    "to was were will with this these those them they their can could should would "
    "may might shall into also been being but do does did not no nor".split()
)


def tokenize(text: str, keep_case: bool = False) -> list[str]:
    """Tokenize text for BM25 indexing."""
    # Strip markdown formatting
    cleaned = MD_STRIP_RE.sub(" ", text)
    # Extract word tokens
    tokens = WORD_RE.findall(cleaned)
    if not keep_case:
        tokens = [t.lower() for t in tokens]
    # Remove stop words
    tokens = [t for t in tokens if t not in STOP_WORDS and len(t) > 1]
    return tokens


def bigrams(tokens: list[str]) -> list[str]:
    """Generate bigram tokens for phrase matching."""
    return [f"{tokens[i]}_{tokens[i+1]}" for i in range(len(tokens) - 1)]


# ─── BM25 Index ──────────────────────────────────────────────────


@dataclass
class ScoredChunk:
    """A chunk with its retrieval score."""

    chunk: Chunk
    score: float
    matched_terms: list[str]


class BM25Index:
    """
    Okapi BM25 index with bigram support and field boosting.

    Usage:
        index = BM25Index()
        index.add_chunks(chunks)
        results = index.search("database schema migration", top_k=5)
    """

    def __init__(self):
        self.chunks: list[Chunk] = []
        # term -> list of (chunk_index, term_frequency)
        self.inverted_index: dict[str, list[tuple[int, float]]] = defaultdict(list)
        self.doc_lengths: list[int] = []
        self.avg_doc_length: float = 0
        self.n_docs: int = 0

    def add_chunks(self, chunks: list[Chunk]) -> None:
        """Index a batch of chunks."""
        self.chunks = chunks
        self.n_docs = len(chunks)
        self.inverted_index.clear()
        self.doc_lengths.clear()

        for idx, chunk in enumerate(chunks):
            # Build weighted token bag from multiple fields
            token_counts = self._build_token_bag(chunk)
            doc_len = sum(token_counts.values())
            self.doc_lengths.append(doc_len)

            for term, freq in token_counts.items():
                self.inverted_index[term].append((idx, freq))

        total_len = sum(self.doc_lengths)
        self.avg_doc_length = total_len / self.n_docs if self.n_docs > 0 else 1

    def _build_token_bag(self, chunk: Chunk) -> Counter:
        """
        Build a weighted bag of tokens from a chunk.

        Tokens from title/headings get boosted weight.
        Tags from frontmatter get boosted weight.
        Bigrams get slight boost for phrase matching.
        """
        bag: Counter = Counter()

        # Content tokens (base weight = 1.0)
        content_tokens = tokenize(chunk.content)
        bag.update(content_tokens)

        # Heading path tokens (boosted)
        for heading in chunk.heading_path:
            h_tokens = tokenize(heading)
            for t in h_tokens:
                bag[t] += TITLE_BOOST

        # Title from frontmatter (boosted)
        title = chunk.frontmatter.get("title", "")
        if title:
            for t in tokenize(str(title)):
                bag[t] += TITLE_BOOST

        # Tags from frontmatter (boosted)
        tags = chunk.frontmatter.get("tags", [])
        if isinstance(tags, list):
            for tag in tags:
                for t in tokenize(str(tag)):
                    bag[t] += TAG_BOOST

        # Bigrams (boosted for phrase matching)
        content_bigrams = bigrams(content_tokens)
        for bg in content_bigrams:
            bag[bg] += BIGRAM_BOOST

        return bag

    def search(
        self,
        query: str,
        top_k: int = 5,
        filter_doc: Optional[str] = None,
        filter_type: Optional[str] = None,
        min_score: float = 0.1,
    ) -> list[ScoredChunk]:
        """
        Search the index with BM25 scoring.

        Args:
            query: Natural language search query
            top_k: Number of results to return
            filter_doc: Only return chunks from this document path
            filter_type: Only return chunks of this type (prose, code, table, etc.)
            min_score: Minimum score threshold
        """
        query_tokens = tokenize(query)
        query_bigrams = bigrams(query_tokens)
        all_query_terms = query_tokens + query_bigrams

        if not all_query_terms:
            return []

        # Score each chunk
        scores: dict[int, float] = defaultdict(float)
        matched: dict[int, list[str]] = defaultdict(list)

        for term in all_query_terms:
            if term not in self.inverted_index:
                continue

            postings = self.inverted_index[term]
            # IDF: log((N - n + 0.5) / (n + 0.5) + 1)
            n = len(postings)
            idf = math.log((self.n_docs - n + 0.5) / (n + 0.5) + 1)

            for chunk_idx, tf in postings:
                # Apply filters
                if filter_doc and self.chunks[chunk_idx].doc_path != filter_doc:
                    continue
                if filter_type and self.chunks[chunk_idx].chunk_type != filter_type:
                    continue

                dl = self.doc_lengths[chunk_idx]
                # BM25: idf * (tf * (k1 + 1)) / (tf + k1 * (1 - b + b * dl/avgdl))
                numerator = tf * (K1 + 1)
                denominator = tf + K1 * (1 - B + B * dl / self.avg_doc_length)
                scores[chunk_idx] += idf * numerator / denominator
                matched[chunk_idx].append(term)

        # Sort by score, apply threshold, return top_k
        results = [
            ScoredChunk(
                chunk=self.chunks[idx],
                score=score,
                matched_terms=list(set(matched[idx])),
            )
            for idx, score in sorted(scores.items(), key=lambda x: -x[1])
            if score >= min_score
        ][:top_k]

        return results

    # ─── Persistence ─────────────────────────────────────────────

    def save(self, path: str | Path) -> None:
        """Save the index to disk as JSON."""
        path = Path(path)
        data = {
            "chunks": [c.to_dict() for c in self.chunks],
            "avg_doc_length": self.avg_doc_length,
            "n_docs": self.n_docs,
        }
        path.write_text(json.dumps(data, indent=2, default=str))

    def load(self, path: str | Path) -> None:
        """Load a saved index and rebuild inverted index."""
        path = Path(path)
        data = json.loads(path.read_text())
        self.chunks = [
            Chunk(
                doc_path=c["doc_path"],
                heading_path=c["heading_path"],
                content=c["content"],
                chunk_type=c["chunk_type"],
                start_line=c["start_line"],
                end_line=c["end_line"],
                frontmatter=c.get("frontmatter", {}),
                tokens_approx=c.get("tokens_approx", 0),
            )
            for c in data["chunks"]
        ]
        # Rebuild the inverted index from loaded chunks
        self.add_chunks(self.chunks)
