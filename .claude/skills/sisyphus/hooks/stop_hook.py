#!/usr/bin/env python3
"""
Sisyphus — Stop Hook

Intercepts Claude Code session exit attempts. When a Sisyphus loop is active
(state file exists), blocks exit and re-injects the original prompt, creating
a self-referential iteration loop.

Hook Protocol:
  Input:  JSON on stdin with transcript/last assistant message
  Output: JSON on stdout with decision ("approve" or "block")
  Exit:   0 = allow exit, 1 = block exit
"""

import json
import re
import sys
from pathlib import Path

STATE_FILE = Path(".claude/sisyphus.local.md")


def parse_frontmatter(content: str) -> dict[str, str]:
    """Parse YAML frontmatter from state file (no pyyaml dependency)."""
    fields: dict[str, str] = {}
    lines = content.split("\n")

    if not lines or lines[0].strip() != "---":
        return fields

    for line in lines[1:]:
        if line.strip() == "---":
            break
        match = re.match(r'^(\w+):\s*(.+)$', line)
        if match:
            key = match.group(1)
            value = match.group(2).strip().strip('"').strip("'")
            fields[key] = value

    return fields


def extract_prompt(content: str) -> str:
    """Extract the prompt body (everything after the YAML frontmatter closing ---)."""
    parts = content.split("---", 2)
    if len(parts) >= 3:
        return parts[2].strip()
    return ""


def approve(reason: str) -> None:
    """Allow exit — output approve JSON and exit 0."""
    print(json.dumps({"decision": "approve", "reason": reason}))
    sys.exit(0)


def block(reason: str, message: str) -> None:
    """Block exit — output block JSON and exit 1."""
    print(json.dumps({"decision": "block", "reason": reason, "message": message}))
    sys.exit(1)


def main() -> None:
    # If no state file exists, loop is not active — allow normal exit
    if not STATE_FILE.exists():
        approve("No active Sisyphus loop")
        return

    content = STATE_FILE.read_text(encoding="utf-8")
    fields = parse_frontmatter(content)

    # If not active, allow exit
    if fields.get("active") != "true":
        approve("Sisyphus loop is not active")
        return

    iteration = int(fields.get("iteration", "1"))
    max_iterations = int(fields.get("max_iterations", "0"))
    completion_promise = fields.get("completion_promise", "")
    if completion_promise == "null":
        completion_promise = ""

    # Check exit condition: max iterations reached
    if max_iterations > 0 and iteration >= max_iterations:
        STATE_FILE.unlink(missing_ok=True)
        approve(f"Sisyphus loop complete — reached max iterations ({iteration}/{max_iterations})")
        return

    # Check exit condition: completion promise found in transcript
    if completion_promise:
        transcript = ""
        if not sys.stdin.isatty():
            try:
                transcript = sys.stdin.read()
            except Exception:
                pass

        promise_tag = f"<promise>{completion_promise}</promise>"
        if promise_tag in transcript:
            STATE_FILE.unlink(missing_ok=True)
            approve("Sisyphus loop complete — completion promise fulfilled")
            return

    # Neither exit condition met — increment iteration and re-inject prompt
    new_iteration = iteration + 1

    # Update iteration counter in state file
    updated = content.replace(f"iteration: {iteration}", f"iteration: {new_iteration}", 1)
    STATE_FILE.write_text(updated, encoding="utf-8")

    # Extract the original prompt
    prompt = extract_prompt(content)

    # Build iteration status
    if max_iterations > 0:
        iter_status = f"Iteration {new_iteration}/{max_iterations}"
    else:
        iter_status = f"Iteration {new_iteration} (no limit)"

    message = (
        f"[Sisyphus — {iter_status}]\n\n"
        f"Review your prior work (check files, git log, test results) then continue:\n\n"
        f"{prompt}"
    )

    block(f"Sisyphus loop continuing — {iter_status}", message)


if __name__ == "__main__":
    main()
