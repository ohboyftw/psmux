"""
ONNX embedding model loader for Engram.

Shares Beacon's all-MiniLM-L6-v2 model files at:
    ~/.claude/skills/beacon/models/all-MiniLM-L6-v2/

Falls back to BM25-only search if ONNX model is unavailable.
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Optional

logger = logging.getLogger("engram.embeddings")

# ─── Optional dependency checks ─────────────────────────────────

_HAS_NUMPY = False
_HAS_ONNX = False
_HAS_TOKENIZERS = False

try:
    import numpy as np
    _HAS_NUMPY = True
except ImportError:
    np = None

try:
    import onnxruntime as ort
    _HAS_ONNX = True
except ImportError:
    ort = None

try:
    from tokenizers import Tokenizer
    _HAS_TOKENIZERS = True
except ImportError:
    Tokenizer = None

# ─── Model paths ────────────────────────────────────────────────

# Shared with Beacon — single model copy on disk
MODELS_DIR = Path.home() / ".claude" / "skills" / "beacon" / "models" / "all-MiniLM-L6-v2"
MODEL_ONNX_PATH = MODELS_DIR / "model.onnx"
TOKENIZER_PATH = MODELS_DIR / "tokenizer.json"

EMBEDDING_DIM = 384
MAX_SEQ_LENGTH = 512


def is_available() -> bool:
    """Check if all dependencies and model files are present."""
    return (
        _HAS_NUMPY
        and _HAS_ONNX
        and _HAS_TOKENIZERS
        and MODEL_ONNX_PATH.exists()
        and TOKENIZER_PATH.exists()
    )


class EmbeddingModel:
    """
    ONNX-based sentence embedding model (all-MiniLM-L6-v2).

    Produces 384-dimensional normalized embeddings suitable for
    cosine similarity search. Lazy-loads model on first encode().
    """

    def __init__(self):
        self._session: Optional[object] = None
        self._tokenizer: Optional[object] = None
        self._loaded = False

    def _load(self):
        """Load ONNX model and tokenizer (lazy, once)."""
        if self._loaded:
            return

        if not is_available():
            raise RuntimeError(
                "ONNX embedding model not available. Ensure onnxruntime, "
                "numpy, and tokenizers are installed, and the model exists at "
                f"{MODELS_DIR}"
            )

        sess_options = ort.SessionOptions()
        sess_options.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_ALL
        sess_options.intra_op_num_threads = 4

        self._session = ort.InferenceSession(
            str(MODEL_ONNX_PATH),
            sess_options=sess_options,
            providers=["CPUExecutionProvider"],
        )

        self._tokenizer = Tokenizer.from_file(str(TOKENIZER_PATH))
        self._tokenizer.enable_truncation(max_length=MAX_SEQ_LENGTH)
        self._tokenizer.enable_padding(length=MAX_SEQ_LENGTH)

        self._loaded = True
        logger.info("ONNX embedding model loaded")

    def encode(self, texts: list[str]) -> "np.ndarray":
        """
        Encode texts into normalized embeddings.

        Args:
            texts: List of strings to encode.

        Returns:
            numpy array of shape (len(texts), 384), L2-normalized.
        """
        self._load()

        if not texts:
            return np.empty((0, EMBEDDING_DIM), dtype=np.float32)

        encoded = self._tokenizer.encode_batch(texts)

        input_ids = np.array([e.ids for e in encoded], dtype=np.int64)
        attention_mask = np.array([e.attention_mask for e in encoded], dtype=np.int64)
        token_type_ids = np.zeros_like(input_ids, dtype=np.int64)

        outputs = self._session.run(
            None,
            {
                "input_ids": input_ids,
                "attention_mask": attention_mask,
                "token_type_ids": token_type_ids,
            },
        )

        # Mean pooling over non-padding tokens
        last_hidden = outputs[0]  # (batch, seq_len, hidden_dim)
        mask_expanded = attention_mask[:, :, np.newaxis].astype(np.float32)
        summed = (last_hidden * mask_expanded).sum(axis=1)
        counts = mask_expanded.sum(axis=1).clip(min=1e-9)
        embeddings = summed / counts

        # L2 normalize for cosine similarity
        norms = np.linalg.norm(embeddings, axis=1, keepdims=True).clip(min=1e-9)
        embeddings = embeddings / norms

        return embeddings.astype(np.float32)

    def encode_single(self, text: str) -> "np.ndarray":
        """Encode a single text, returns shape (384,)."""
        return self.encode([text])[0]
