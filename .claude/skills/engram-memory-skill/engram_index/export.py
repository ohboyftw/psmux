"""
Export and import portable memory snapshots.

Snapshots are JSON files that can be shared between machines,
teammates, or used for backup/restore.
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

logger = logging.getLogger("engram.export")


def export_snapshot(
    engram,  # EngramMemory instance
    output_path: Optional[str | Path] = None,
) -> Path:
    """
    Export all project memories to a portable JSON snapshot.

    Args:
        engram: EngramMemory instance
        output_path: Where to write the snapshot (default: .engram/export/)
    """
    if output_path is None:
        timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        output_path = engram.config.export_dir / f"snapshot-{timestamp}.json"
    else:
        output_path = Path(output_path)

    user_id = engram._scope_to_user_id("project")

    # Get all memories from store
    try:
        all_records = engram.store.get_all(user_id=user_id)
        memories = [r.to_dict() for r in all_records]
    except Exception as e:
        logger.warning(f"Could not export memories: {e}")
        memories = []

    # Get traces from history DB
    traces = engram.history.get_traces(limit=1000)

    # Get history stats
    stats = engram.history.get_stats()

    snapshot = {
        "version": "2.0",
        "exported_at": datetime.now(timezone.utc).isoformat(),
        "project_name": engram.config.project_name,
        "project_hash": engram.config.project_hash,
        "memories": memories,
        "traces": traces,
        "stats": stats,
    }

    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(json.dumps(snapshot, indent=2, default=str))
    logger.info(f"Exported {len(memories)} memories and {len(traces)} traces to {output_path}")

    return output_path


def import_snapshot(
    engram,  # EngramMemory instance
    input_path: str | Path,
    merge: bool = True,
) -> dict:
    """
    Import memories from a portable JSON snapshot.

    Args:
        engram: EngramMemory instance
        input_path: Path to the snapshot file
        merge: If True, add to existing memories. If False, would need clear first.
    """
    input_path = Path(input_path)
    snapshot = json.loads(input_path.read_text())

    imported = {"memories": 0, "traces": 0, "errors": 0}

    # Import memories
    for mem in snapshot.get("memories", []):
        try:
            content = mem.get("memory", mem.get("content", mem.get("text", "")))
            if not content:
                continue

            metadata = mem.get("metadata", mem.get("extra_metadata", {}))
            engram.remember(
                content,
                memory_type=metadata.get("memory_type", mem.get("memory_type", "fact")),
                scope=metadata.get("scope", mem.get("scope", "project")),
                metadata={**metadata, "imported_from": str(input_path)},
            )
            imported["memories"] += 1
        except Exception as e:
            logger.warning(f"Failed to import memory: {e}")
            imported["errors"] += 1

    # Import traces
    for trace in snapshot.get("traces", []):
        try:
            reasoning = trace.get("reasoning", "[]")
            if isinstance(reasoning, str):
                reasoning = json.loads(reasoning)

            tools = trace.get("tools_used", "[]")
            if isinstance(tools, str):
                tools = json.loads(tools)

            entities = trace.get("related_entities", "[]")
            if isinstance(entities, str):
                entities = json.loads(entities)

            engram.history.log_trace(
                task=trace.get("task", "imported"),
                reasoning=reasoning,
                outcome=trace.get("outcome", "unknown"),
                tools_used=tools,
                duration_minutes=trace.get("duration_minutes"),
                related_entities=entities,
            )
            imported["traces"] += 1
        except Exception as e:
            logger.warning(f"Failed to import trace: {e}")
            imported["errors"] += 1

    logger.info(
        f"Imported {imported['memories']} memories and {imported['traces']} traces "
        f"({imported['errors']} errors)"
    )
    return imported
