"""
Semantic vector retriever for the Beacon knowledge base.

Uses ONNX Runtime + tokenizers for dense embeddings with cosine similarity.
Falls back gracefully to BM25-only if onnxruntime is not installed.

Embedding model: all-MiniLM-L6-v2 (fast, 384-dim, good for retrieval)
Storage: numpy .npz files (no external vector DB needed)
"""

from __future__ import annotations

import json
import logging
from dataclasses import dataclass
from pathlib import Path
from typing import Optional

from .chunker import Chunk

logger = logging.getLogger("beacon.semantic")

# ─── Check for optional dependencies ────────────────────────────

_HAS_ORT = False
_HAS_TOKENIZERS = False
_HAS_NUMPY = False

try:
    import numpy as np

    _HAS_NUMPY = True
except ImportError:
    np = None

try:
    import onnxruntime as ort

    _HAS_ORT = True
except ImportError:
    ort = None

try:
    from tokenizers import Tokenizer

    _HAS_TOKENIZERS = True
except ImportError:
    Tokenizer = None


def is_available() -> bool:
    """Check if semantic search dependencies are installed."""
    return _HAS_ORT and _HAS_TOKENIZERS and _HAS_NUMPY


def install_hint() -> str:
    """Return installation instructions."""
    missing = []
    if not _HAS_NUMPY:
        missing.append("numpy")
    if not _HAS_ORT:
        missing.append("onnxruntime")
    if not _HAS_TOKENIZERS:
        missing.append("tokenizers")
    return f"pip install {' '.join(missing)} --break-system-packages"


# ─── Model paths ────────────────────────────────────────────────

MODELS_DIR = Path(__file__).resolve().parent.parent / "models" / "all-MiniLM-L6-v2"
MODEL_ONNX_PATH = MODELS_DIR / "model.onnx"
TOKENIZER_PATH = MODELS_DIR / "tokenizer.json"


def model_is_downloaded() -> bool:
    """Check if the ONNX model files are present."""
    return MODEL_ONNX_PATH.exists() and TOKENIZER_PATH.exists()


# ─── Semantic Index ──────────────────────────────────────────────

DEFAULT_MODEL = "all-MiniLM-L6-v2"


@dataclass
class SemanticResult:
    """A chunk with its cosine similarity score."""

    chunk: Chunk
    score: float


class SemanticIndex:
    """
    Dense vector index using ONNX Runtime.

    Usage:
        index = SemanticIndex()
        index.build(chunks)
        results = index.search("how does auth work", top_k=5)
    """

    def __init__(self, model_name: str = DEFAULT_MODEL):
        if not is_available():
            raise ImportError(
                f"Semantic search requires additional packages.\n"
                f"Install with: {install_hint()}"
            )

        if not model_is_downloaded():
            raise FileNotFoundError(
                f"ONNX model not found at {MODELS_DIR}.\n"
                f"Run: py ~/.claude/skills/beacon/scripts/download_onnx_model.py"
            )

        self.model_name = model_name
        self._session = None  # Lazy load
        self._tokenizer = None  # Lazy load
        self.chunks: list[Chunk] = []
        self.embeddings: Optional[np.ndarray] = None  # (n_chunks, dim)

    def _load_model(self):
        if self._session is None:
            logger.info(f"Loading ONNX embedding model: {self.model_name}")

            # Use CPU execution provider (fast, no GPU deps)
            sess_options = ort.SessionOptions()
            sess_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
            sess_options.intra_op_num_threads = 4

            self._session = ort.InferenceSession(
                str(MODEL_ONNX_PATH),
                sess_options=sess_options,
                providers=["CPUExecutionProvider"],
            )
            self._tokenizer = Tokenizer.from_file(str(TOKENIZER_PATH))

            # Configure tokenizer for batch encoding
            self._tokenizer.enable_truncation(max_length=512)
            self._tokenizer.enable_padding(length=512)

            logger.info("ONNX model loaded.")

    def _encode(self, texts: list[str]) -> np.ndarray:
        """
        Encode texts to normalized embeddings using ONNX Runtime.

        Performs: tokenize -> ONNX inference -> mean pooling -> L2 normalize.
        Output: (N, 384) float32 array, L2-normalized.
        """
        self._load_model()

        if isinstance(texts, str):
            texts = [texts]

        # Tokenize
        encodings = self._tokenizer.encode_batch(texts)

        input_ids = np.array([e.ids for e in encodings], dtype=np.int64)
        attention_mask = np.array([e.attention_mask for e in encodings], dtype=np.int64)
        token_type_ids = np.zeros_like(input_ids, dtype=np.int64)

        # Run ONNX inference
        outputs = self._session.run(
            None,
            {
                "input_ids": input_ids,
                "attention_mask": attention_mask,
                "token_type_ids": token_type_ids,
            },
        )

        # outputs[0] = last_hidden_state: (batch, seq_len, hidden_dim)
        last_hidden = outputs[0]

        # Mean pooling: average over non-padding tokens
        mask_expanded = attention_mask[:, :, np.newaxis].astype(np.float32)
        sum_embeddings = np.sum(last_hidden * mask_expanded, axis=1)
        sum_mask = np.sum(mask_expanded, axis=1)
        sum_mask = np.clip(sum_mask, a_min=1e-9, a_max=None)
        mean_pooled = sum_embeddings / sum_mask

        # L2 normalize
        norms = np.linalg.norm(mean_pooled, axis=1, keepdims=True)
        norms = np.clip(norms, a_min=1e-9, a_max=None)
        normalized = mean_pooled / norms

        return normalized

    def _chunk_to_text(self, chunk: Chunk) -> str:
        """
        Convert chunk to embedding-friendly text.

        Prepend heading path and title for better semantic matching.
        """
        parts = []

        # Heading context
        if chunk.heading_path:
            parts.append(" > ".join(chunk.heading_path))

        # Title from frontmatter
        title = chunk.frontmatter.get("title", "")
        if title:
            parts.append(str(title))

        # Tags
        tags = chunk.frontmatter.get("tags", [])
        if isinstance(tags, list) and tags:
            parts.append("Tags: " + ", ".join(str(t) for t in tags))

        # Content
        parts.append(chunk.content)

        return "\n".join(parts)

    def build(self, chunks: list[Chunk], batch_size: int = 64) -> None:
        """
        Build the vector index from chunks.

        Args:
            chunks: List of Chunk objects to index
            batch_size: Encoding batch size
        """
        self._load_model()
        self.chunks = chunks

        texts = [self._chunk_to_text(c) for c in chunks]
        logger.info(f"Encoding {len(texts)} chunks...")

        # Batch encode for memory efficiency
        all_embeddings = []
        for i in range(0, len(texts), batch_size):
            batch = texts[i : i + batch_size]
            batch_emb = self._encode(batch)
            all_embeddings.append(batch_emb)

        self.embeddings = np.vstack(all_embeddings) if all_embeddings else np.empty((0, 384))
        logger.info(f"Index built: {self.embeddings.shape}")

    def search(
        self,
        query: str,
        top_k: int = 5,
        filter_doc: Optional[str] = None,
        filter_type: Optional[str] = None,
        min_score: float = 0.2,
    ) -> list[SemanticResult]:
        """
        Search using cosine similarity.

        Args:
            query: Natural language query
            top_k: Number of results
            filter_doc: Restrict to specific document
            filter_type: Restrict to chunk type
            min_score: Minimum cosine similarity threshold
        """
        if self.embeddings is None or len(self.chunks) == 0:
            return []

        # Encode query (returns (1, dim), take first row)
        query_vec = self._encode(query)[0]

        # Cosine similarity (embeddings are normalized, so dot product = cosine)
        similarities = self.embeddings @ query_vec

        # Apply filters by zeroing out filtered indices
        if filter_doc or filter_type:
            mask = np.ones(len(self.chunks), dtype=bool)
            for i, chunk in enumerate(self.chunks):
                if filter_doc and chunk.doc_path != filter_doc:
                    mask[i] = False
                if filter_type and chunk.chunk_type != filter_type:
                    mask[i] = False
            similarities = similarities * mask

        # Get top_k indices
        top_indices = np.argsort(similarities)[::-1][:top_k]

        results = []
        for idx in top_indices:
            score = float(similarities[idx])
            if score < min_score:
                break
            results.append(
                SemanticResult(chunk=self.chunks[idx], score=score)
            )

        return results

    # ─── Persistence ─────────────────────────────────────────────

    def save(self, directory: str | Path) -> None:
        """Save embeddings and chunk metadata."""
        directory = Path(directory)
        directory.mkdir(parents=True, exist_ok=True)

        # Save embeddings as numpy
        np.save(directory / "embeddings.npy", self.embeddings)

        # Save chunk metadata
        meta = {
            "model_name": self.model_name,
            "model_variant": "onnx",
            "n_chunks": len(self.chunks),
            "embedding_dim": self.embeddings.shape[1] if self.embeddings is not None else 0,
            "chunks": [c.to_dict() for c in self.chunks],
        }
        (directory / "meta.json").write_text(json.dumps(meta, indent=2, default=str))

    def load(self, directory: str | Path) -> None:
        """Load saved embeddings and rebuild index."""
        directory = Path(directory)

        meta = json.loads((directory / "meta.json").read_text())
        self.model_name = meta["model_name"]
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
            for c in meta["chunks"]
        ]
        self.embeddings = np.load(directory / "embeddings.npy")
