#!/usr/bin/env python3
"""
Sisyphus — Setup Script

Parses arguments from /sisyphus command, validates inputs, creates the
state file that activates the stop hook loop.

Usage:
    setup_sisyphus.py PROMPT... [--max-iterations N] [--completion-promise TEXT]
    setup_sisyphus.py -h|--help
"""

import argparse
import sys
from datetime import datetime, timezone
from pathlib import Path

STATE_DIR = Path(".claude")
STATE_FILE = STATE_DIR / "sisyphus.local.md"


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Sisyphus — Iterative self-referential development loop",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""\
Examples:
  /sisyphus Build a REST API with tests --max-iterations 20

  /sisyphus Implement auth with JWT. Output <promise>DONE</promise> when complete \\
    --completion-promise "DONE" --max-iterations 15

Safety:
  Always set --max-iterations as your primary safety mechanism.
  Use /cancel-sisyphus to manually stop an active loop.""",
    )
    parser.add_argument(
        "prompt", nargs="*", help="Task description (multi-word, no quotes needed)"
    )
    parser.add_argument(
        "--max-iterations",
        type=int,
        default=0,
        help="Maximum iterations before auto-stop (0 = unlimited)",
    )
    parser.add_argument(
        "--completion-promise",
        type=str,
        default="",
        help="Promise phrase — loop exits when agent outputs <promise>TEXT</promise>",
    )

    args = parser.parse_args()

    # Check for existing loop
    if STATE_FILE.exists():
        print()
        print("  A Sisyphus loop is already active!")
        print("   Use /cancel-sisyphus to stop it before starting a new one.")
        print()
        content = STATE_FILE.read_text(encoding="utf-8")
        preview = "\n".join(content.split("\n")[:20])
        print("Current state:")
        print(preview)
        sys.exit(1)

    # Validate prompt
    if not args.prompt:
        print("  Error: No prompt provided")
        print()
        print("Usage: /sisyphus PROMPT [--max-iterations N] [--completion-promise TEXT]")
        print("Run with -h for full help")
        sys.exit(1)

    prompt = " ".join(args.prompt)
    max_iterations = args.max_iterations
    completion_promise = args.completion_promise

    if max_iterations < 0:
        print("  Error: --max-iterations must be >= 0")
        sys.exit(1)

    # Prepare state file
    started_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    promise_yaml = f'"{completion_promise}"' if completion_promise else "null"

    STATE_DIR.mkdir(parents=True, exist_ok=True)

    state_content = f"""\
---
active: true
iteration: 1
max_iterations: {max_iterations}
completion_promise: {promise_yaml}
started_at: "{started_at}"
---

{prompt}
"""

    STATE_FILE.write_text(state_content, encoding="utf-8")

    # Display activation status
    prompt_display = prompt[:80] + ("..." if len(prompt) > 80 else "")
    max_display = str(max_iterations) if max_iterations > 0 else "unlimited"

    print()
    print("=" * 59)
    print("  Sisyphus Loop ACTIVATED")
    print("=" * 59)
    print()
    print(f"  Prompt:     {prompt_display}")
    print(f"  Max iter:   {max_display}")

    if completion_promise:
        print(f'  Promise:    "{completion_promise}"')

    print(f"  Started:    {started_at}")
    print(f"  State file: {STATE_FILE}")
    print()

    # Safety warnings
    if max_iterations == 0 and not completion_promise:
        print("=" * 59)
        print("  WARNING: No exit conditions set!")
        print("=" * 59)
        print()
        print("  This loop will run INDEFINITELY.")
        print("  Use /cancel-sisyphus to stop manually.")
        print("  Consider adding --max-iterations for safety.")
        print()

    # Completion promise instructions
    if completion_promise:
        print("=" * 59)
        print("  COMPLETION PROMISE")
        print("=" * 59)
        print()
        print("  To complete this loop, output this EXACT text:")
        print()
        print(f"    <promise>{completion_promise}</promise>")
        print()
        print("  STRICT REQUIREMENTS:")
        print("    - Use <promise> XML tags EXACTLY as shown above")
        print("    - The statement MUST be completely and unequivocally TRUE")
        print("    - Do NOT output false statements to exit the loop")
        print("    - If stuck, document what's blocking and keep trying")
        print()

    # Monitoring hint
    print("-" * 59)
    print(f"  Monitor:  cat {STATE_FILE}")
    print("  Cancel:   /cancel-sisyphus")
    print("-" * 59)
    print()
    print("Loop is now active. Begin working on the task.")


if __name__ == "__main__":
    main()
