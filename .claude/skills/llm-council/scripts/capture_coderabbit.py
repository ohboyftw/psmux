#!/usr/bin/env python3
"""Capture CodeRabbit results for benchmarking. Run with --mode template to create fillable JSONs."""
import asyncio, json, sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).parent.parent))
from evaluate import BUILTIN_CASES

OUTPUT_DIR = Path("eval_data/coderabbit_captures")

def create_template(case_id=None):
    cases = [c for c in BUILTIN_CASES if c.id == case_id] if case_id else BUILTIN_CASES
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    for tc in cases:
        template = {
            "test_case_id": tc.id, "model": "coderabbit-manual",
            "findings": [{"severity": "critical|major|minor|suggestion",
                "category": "security|performance|correctness|style|architecture",
                "file": "path/to/file", "line": None,
                "title": "Finding title", "description": "Description", "suggestion": "Fix"}],
            "summary": "", "confidence": 0.85, "latency_ms": 0}
        out = OUTPUT_DIR / f"{tc.id}.template.json"
        out.write_text(json.dumps(template, indent=2))
        print(f"📄 {out}")
    print(f"\nFill in findings and rename .template.json → .json")

if __name__ == "__main__":
    import argparse
    p = argparse.ArgumentParser()
    p.add_argument("--mode", choices=["template", "manual"], default="template")
    p.add_argument("--case")
    args = p.parse_args()
    create_template(args.case)
