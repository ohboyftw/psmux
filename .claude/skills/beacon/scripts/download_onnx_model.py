#!/usr/bin/env python3
"""
Download the ONNX model files for Beacon's semantic search.

Downloads all-MiniLM-L6-v2 in ONNX format from HuggingFace Hub.
Files are stored at: ~/.claude/skills/beacon/models/all-MiniLM-L6-v2/

Required: pip install huggingface-hub
"""

import sys
from pathlib import Path

REPO_ID = "Qdrant/all-MiniLM-L6-v2-onnx"
TARGET_DIR = Path(__file__).resolve().parent / "models" / "all-MiniLM-L6-v2"

FILES_TO_DOWNLOAD = [
    "model.onnx",
    "tokenizer.json",
]


def main():
    try:
        from huggingface_hub import hf_hub_download
    except ImportError:
        print("Error: huggingface-hub not installed.")
        print("Install with: pip install huggingface-hub --break-system-packages")
        sys.exit(1)

    TARGET_DIR.mkdir(parents=True, exist_ok=True)
    print(f"Downloading ONNX model to: {TARGET_DIR}")

    for filename in FILES_TO_DOWNLOAD:
        dest = TARGET_DIR / filename
        if dest.exists():
            print(f"  {filename}: already exists, skipping")
            continue

        print(f"  {filename}: downloading from {REPO_ID}...")
        downloaded = hf_hub_download(
            repo_id=REPO_ID,
            filename=filename,
            local_dir=str(TARGET_DIR),
        )
        print(f"  {filename}: done ({Path(downloaded).stat().st_size / 1024 / 1024:.1f} MB)")

    # Verify
    model_path = TARGET_DIR / "model.onnx"
    tokenizer_path = TARGET_DIR / "tokenizer.json"

    if model_path.exists() and tokenizer_path.exists():
        model_mb = model_path.stat().st_size / 1024 / 1024
        print(f"\nModel ready: {model_mb:.1f} MB")
        print(f"Location: {TARGET_DIR}")

        # Quick smoke test
        try:
            import onnxruntime as ort
            from tokenizers import Tokenizer
            import numpy as np

            sess = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
            tok = Tokenizer.from_file(str(tokenizer_path))
            tok.enable_truncation(max_length=512)
            tok.enable_padding(length=512)

            enc = tok.encode("test sentence")
            input_ids = np.array([enc.ids], dtype=np.int64)
            attention_mask = np.array([enc.attention_mask], dtype=np.int64)
            token_type_ids = np.zeros_like(input_ids, dtype=np.int64)

            outputs = sess.run(None, {
                "input_ids": input_ids,
                "attention_mask": attention_mask,
                "token_type_ids": token_type_ids,
            })
            dim = outputs[0].shape[-1]
            print(f"Smoke test passed: embedding dim = {dim}")
        except Exception as e:
            print(f"Warning: Smoke test failed: {e}")
    else:
        print("\nError: Not all files downloaded.")
        sys.exit(1)


if __name__ == "__main__":
    main()
