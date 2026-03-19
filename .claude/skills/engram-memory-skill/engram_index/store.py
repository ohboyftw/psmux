"""
SQLite memory store for Engram.

Single source of truth for all memories. Replaces chromadb with
a lightweight SQLite backend supporting:
- CRUD operations
- SHA256-based deduplication
- Scope/type filtering
- Full-text search fallback
"""

from __future__ import annotations

import hashlib
import json
import logging
import sqlite3
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

logger = logging.getLogger("engram.store")


# ─── Data Types ─────────────────────────────────────────────────


@dataclass
class MemoryRecord:
    """A single stored memory."""
    id: str
    content: str
    memory_type: str  # fact, decision, pattern, preference, context, trace
    scope: str        # project, session, user
    user_id: str
    project_name: str = ""
    project_hash: str = ""
    created_at: str = ""
    tags: list[str] = field(default_factory=list)
    extra_metadata: dict = field(default_factory=dict)
    content_hash: str = ""

    def to_dict(self) -> dict:
        return {
            "id": self.id,
            "content": self.content,
            "memory_type": self.memory_type,
            "scope": self.scope,
            "user_id": self.user_id,
            "project_name": self.project_name,
            "project_hash": self.project_hash,
            "created_at": self.created_at,
            "tags": self.tags,
            "extra_metadata": self.extra_metadata,
            "content_hash": self.content_hash,
        }

    @staticmethod
    def from_row(row: sqlite3.Row) -> "MemoryRecord":
        return MemoryRecord(
            id=row["id"],
            content=row["content"],
            memory_type=row["memory_type"],
            scope=row["scope"],
            user_id=row["user_id"],
            project_name=row["project_name"] or "",
            project_hash=row["project_hash"] or "",
            created_at=row["created_at"],
            tags=json.loads(row["tags"]) if row["tags"] else [],
            extra_metadata=json.loads(row["extra_metadata"]) if row["extra_metadata"] else {},
            content_hash=row["content_hash"] or "",
        )


def _content_hash(content: str) -> str:
    """SHA256 hash for dedup."""
    return hashlib.sha256(content.strip().encode("utf-8")).hexdigest()


# ─── Memory Store ───────────────────────────────────────────────


class MemoryStore:
    """
    SQLite-backed memory store.

    All memories for a project live in .engram/memories.db.
    Dedup via content SHA256 — same content string = same memory.
    """

    def __init__(self, db_path: Path):
        self.db_path = db_path
        self._init_db()

    def _init_db(self):
        with self._connect() as conn:
            conn.execute("""
                CREATE TABLE IF NOT EXISTS memories (
                    id TEXT PRIMARY KEY,
                    content TEXT NOT NULL,
                    memory_type TEXT NOT NULL,
                    scope TEXT NOT NULL,
                    user_id TEXT NOT NULL,
                    project_name TEXT,
                    project_hash TEXT,
                    created_at TEXT NOT NULL,
                    tags TEXT,
                    extra_metadata TEXT,
                    content_hash TEXT
                )
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_memories_user
                ON memories(user_id)
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_memories_hash
                ON memories(content_hash)
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_memories_type
                ON memories(memory_type)
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_memories_scope
                ON memories(scope)
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_memories_created_at
                ON memories(created_at)
            """)

    def _connect(self) -> sqlite3.Connection:
        conn = sqlite3.connect(str(self.db_path))
        conn.row_factory = sqlite3.Row
        return conn

    # ─── CRUD ───────────────────────────────────────────────────

    def add(
        self,
        content: str,
        memory_type: str,
        scope: str,
        user_id: str,
        project_name: str = "",
        project_hash: str = "",
        tags: list[str] | None = None,
        extra_metadata: dict | None = None,
    ) -> MemoryRecord:
        """
        Add a memory. Deduplicates by content hash + user_id.

        Returns the existing record if duplicate, or the new record.
        """
        c_hash = _content_hash(content)

        # Check for duplicate
        with self._connect() as conn:
            existing = conn.execute(
                "SELECT * FROM memories WHERE content_hash = ? AND user_id = ?",
                (c_hash, user_id),
            ).fetchone()

            if existing:
                logger.debug(f"Dedup hit: content_hash={c_hash[:12]}")
                return MemoryRecord.from_row(existing)

        # Insert new
        record = MemoryRecord(
            id=str(uuid.uuid4()),
            content=content[:2000],  # Truncate to 2000 chars
            memory_type=memory_type,
            scope=scope,
            user_id=user_id,
            project_name=project_name,
            project_hash=project_hash,
            created_at=datetime.now(timezone.utc).isoformat(),
            tags=tags or [],
            extra_metadata=extra_metadata or {},
            content_hash=c_hash,
        )

        with self._connect() as conn:
            conn.execute(
                "INSERT INTO memories (id, content, memory_type, scope, user_id, "
                "project_name, project_hash, created_at, tags, extra_metadata, content_hash) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    record.id,
                    record.content,
                    record.memory_type,
                    record.scope,
                    record.user_id,
                    record.project_name,
                    record.project_hash,
                    record.created_at,
                    json.dumps(record.tags),
                    json.dumps(record.extra_metadata),
                    record.content_hash,
                ),
            )

        logger.debug(f"Stored memory {record.id[:8]} ({memory_type}/{scope})")
        return record

    def get(self, memory_id: str) -> Optional[MemoryRecord]:
        """Get a single memory by ID."""
        with self._connect() as conn:
            row = conn.execute(
                "SELECT * FROM memories WHERE id = ?", (memory_id,)
            ).fetchone()
            return MemoryRecord.from_row(row) if row else None

    def get_all(
        self,
        user_id: Optional[str] = None,
        scope: Optional[str] = None,
        memory_type: Optional[str] = None,
        limit: int = 1000,
        since: Optional[str] = None,
        until: Optional[str] = None,
    ) -> list[MemoryRecord]:
        """Get all memories with optional filters.

        Args:
            since: ISO 8601 date/datetime lower bound (inclusive). Empty string ignored.
            until: ISO 8601 date/datetime upper bound (inclusive). Empty string ignored.
        """
        conditions = []
        params = []

        if user_id:
            conditions.append("user_id = ?")
            params.append(user_id)
        if scope:
            conditions.append("scope = ?")
            params.append(scope)
        if memory_type:
            conditions.append("memory_type = ?")
            params.append(memory_type)
        if since:
            conditions.append("created_at >= ?")
            params.append(since)
        if until:
            conditions.append("created_at <= ?")
            params.append(until)

        where = f"WHERE {' AND '.join(conditions)}" if conditions else ""

        with self._connect() as conn:
            rows = conn.execute(
                f"SELECT * FROM memories {where} ORDER BY created_at DESC LIMIT ?",
                (*params, limit),
            ).fetchall()
            return [MemoryRecord.from_row(r) for r in rows]

    def delete(self, memory_id: str) -> bool:
        """Delete a memory by ID."""
        with self._connect() as conn:
            cursor = conn.execute(
                "DELETE FROM memories WHERE id = ?", (memory_id,)
            )
            return cursor.rowcount > 0

    def count(self, user_id: Optional[str] = None) -> int:
        """Count memories, optionally filtered by user_id."""
        if user_id:
            with self._connect() as conn:
                row = conn.execute(
                    "SELECT COUNT(*) as cnt FROM memories WHERE user_id = ?",
                    (user_id,),
                ).fetchone()
                return row["cnt"]
        else:
            with self._connect() as conn:
                row = conn.execute("SELECT COUNT(*) as cnt FROM memories").fetchone()
                return row["cnt"]

    def count_by_type(self, user_id: Optional[str] = None) -> dict[str, int]:
        """Count memories grouped by type."""
        if user_id:
            query = "SELECT memory_type, COUNT(*) as cnt FROM memories WHERE user_id = ? GROUP BY memory_type"
            params: tuple = (user_id,)
        else:
            query = "SELECT memory_type, COUNT(*) as cnt FROM memories GROUP BY memory_type"
            params = ()

        with self._connect() as conn:
            rows = conn.execute(query, params).fetchall()
            return {row["memory_type"]: row["cnt"] for row in rows}
