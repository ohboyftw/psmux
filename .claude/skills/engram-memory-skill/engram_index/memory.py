"""
EngramMemory: Main memory class using local SQLite + BM25/ONNX search.

Provides:
- remember() — store memories with type and scope metadata
- recall() — search memory with scope-aware ranking
- relate() — traverse entity graph
- learn_from_conversation() — store conversation context
- bridge_recall() — combined Engram + Serena search
- trace() — store reasoning traces

All local. Zero API calls. <200ms per operation.
"""

from __future__ import annotations

import json
import logging
import os
import sqlite3
from datetime import datetime, timezone
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

from .config import EngramConfig, load_config

logger = logging.getLogger("engram.memory")


# ─── Serena Skill Resolution ────────────────────────────────────
# Serena is installed as a user-level Claude Code skill. Its Python
# package (serena_index) lives inside the skill's scripts/ directory,
# NOT on the default sys.path. We resolve it at runtime.

_serena_retriever_class = None
_serena_resolve_attempted = False


def _resolve_serena_import():
    """
    Try to import serena_index.retriever from multiple locations:
    1. Already on sys.path (e.g. PYTHONPATH set)
    2. User-level Claude Code skills directory
    3. Common skill install locations
    """
    global _serena_retriever_class, _serena_resolve_attempted
    if _serena_resolve_attempted:
        return _serena_retriever_class
    _serena_resolve_attempted = True

    # Strategy 1: Direct import (already on path)
    try:
        from serena_index.retriever import HybridRetriever
        _serena_retriever_class = HybridRetriever
        return _serena_retriever_class
    except ImportError:
        pass

    # Strategy 2: Scan known skill directories
    import sys
    skill_search_paths = [
        Path.home() / ".claude" / "skills" / "serena-knowledge-system" / "scripts",
        Path.home() / ".claude" / "skills" / "user" / "serena-knowledge-system" / "scripts",
        Path("/mnt/skills/user/serena-knowledge-system/scripts"),
        Path("/mnt/skills/public/serena-knowledge-system/scripts"),
        Path("/mnt/skills/private/serena-knowledge-system/scripts"),
    ]

    # Also check SERENA_SKILL_PATH env var
    env_path = os.environ.get("SERENA_SKILL_PATH")
    if env_path:
        skill_search_paths.insert(0, Path(env_path) / "scripts")

    for skill_path in skill_search_paths:
        if skill_path.exists() and (skill_path / "serena_index").is_dir():
            sys.path.insert(0, str(skill_path))
            try:
                from serena_index.retriever import HybridRetriever
                _serena_retriever_class = HybridRetriever
                logger.debug(f"Serena found at {skill_path}")
                return _serena_retriever_class
            except ImportError:
                sys.path.remove(str(skill_path))

    logger.debug("Serena skill not found — bridge mode will return Engram-only results")
    return None


def _get_serena_retriever(project_root: Path):
    """Get an initialized Serena retriever for a project, or None."""
    cls = _resolve_serena_import()
    if cls is None:
        return None

    serena_index_dir = project_root / ".serena"
    if not serena_index_dir.exists():
        return None

    retriever = cls(project_root, use_semantic=False)

    if (serena_index_dir / "bm25_index.json").exists():
        retriever.load_index(serena_index_dir)
    else:
        retriever.build_index()

    return retriever


# ─── Result Types ────────────────────────────────────────────────


@dataclass
class MemoryResult:
    """A single memory retrieval result."""
    memory: str
    memory_id: str
    score: float
    metadata: dict = field(default_factory=dict)
    source: str = "engram"  # "engram" or "serena"

    @property
    def memory_type(self) -> str:
        return self.metadata.get("memory_type", "unknown")

    @property
    def scope(self) -> str:
        return self.metadata.get("scope", "project")

    @property
    def timestamp(self) -> str:
        return self.metadata.get("created_at", "")


@dataclass
class BridgeResult:
    """Combined results from Engram + Serena."""
    engram_results: list[MemoryResult] = field(default_factory=list)
    serena_results: list = field(default_factory=list)  # serena RetrievalResult
    graph_edges: list[dict] = field(default_factory=list)

    @property
    def total(self) -> int:
        return len(self.engram_results) + len(self.serena_results)


# ─── History Database ────────────────────────────────────────────


class HistoryDB:
    """SQLite audit trail for all memory operations."""

    def __init__(self, db_path: Path):
        self.db_path = db_path
        self._init_db()

    def _init_db(self):
        db_path = str(self.db_path)
        with sqlite3.connect(db_path) as conn:
            conn.execute("""
                CREATE TABLE IF NOT EXISTS memory_history (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    operation TEXT NOT NULL,
                    memory_type TEXT,
                    scope TEXT,
                    content_preview TEXT,
                    memory_id TEXT,
                    metadata TEXT
                )
            """)
            conn.execute("""
                CREATE TABLE IF NOT EXISTS traces (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    timestamp TEXT NOT NULL,
                    task TEXT NOT NULL,
                    reasoning TEXT NOT NULL,
                    outcome TEXT,
                    tools_used TEXT,
                    duration_minutes REAL,
                    related_entities TEXT,
                    memory_ids TEXT
                )
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_history_timestamp
                ON memory_history(timestamp)
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_traces_timestamp
                ON traces(timestamp)
            """)

    def log_operation(self, operation: str, memory_type: str, scope: str,
                      content_preview: str, memory_id: str = "", metadata: dict = None):
        with sqlite3.connect(str(self.db_path)) as conn:
            conn.execute(
                "INSERT INTO memory_history (timestamp, operation, memory_type, scope, "
                "content_preview, memory_id, metadata) VALUES (?, ?, ?, ?, ?, ?, ?)",
                (
                    datetime.now(timezone.utc).isoformat(),
                    operation,
                    memory_type,
                    scope,
                    content_preview[:200],
                    memory_id,
                    json.dumps(metadata or {}),
                ),
            )

    def log_trace(self, task: str, reasoning: list, outcome: str,
                  tools_used: list = None, duration_minutes: float = None,
                  related_entities: list = None, memory_ids: list = None):
        with sqlite3.connect(str(self.db_path)) as conn:
            conn.execute(
                "INSERT INTO traces (timestamp, task, reasoning, outcome, tools_used, "
                "duration_minutes, related_entities, memory_ids) VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    datetime.now(timezone.utc).isoformat(),
                    task,
                    json.dumps(reasoning),
                    outcome,
                    json.dumps(tools_used or []),
                    duration_minutes,
                    json.dumps(related_entities or []),
                    json.dumps(memory_ids or []),
                ),
            )

    def get_traces(self, task_query: str = None, limit: int = 10) -> list[dict]:
        with sqlite3.connect(str(self.db_path)) as conn:
            conn.row_factory = sqlite3.Row
            if task_query:
                rows = conn.execute(
                    "SELECT * FROM traces WHERE task LIKE ? ORDER BY timestamp DESC LIMIT ?",
                    (f"%{task_query}%", limit),
                ).fetchall()
            else:
                rows = conn.execute(
                    "SELECT * FROM traces ORDER BY timestamp DESC LIMIT ?",
                    (limit,),
                ).fetchall()
            return [dict(r) for r in rows]

    def get_stats(self) -> dict:
        with sqlite3.connect(str(self.db_path)) as conn:
            total = conn.execute("SELECT COUNT(*) FROM memory_history").fetchone()[0]
            by_type = conn.execute(
                "SELECT memory_type, COUNT(*) FROM memory_history GROUP BY memory_type"
            ).fetchall()
            by_op = conn.execute(
                "SELECT operation, COUNT(*) FROM memory_history GROUP BY operation"
            ).fetchall()
            traces = conn.execute("SELECT COUNT(*) FROM traces").fetchone()[0]
            return {
                "total_operations": total,
                "by_type": dict(by_type),
                "by_operation": dict(by_op),
                "total_traces": traces,
            }


# ─── Main Memory Class ──────────────────────────────────────────


class EngramMemory:
    """
    Dynamic memory layer using local SQLite + BM25/ONNX search.

    Zero API calls. All local. <200ms per operation.

    Usage:
        engram = EngramMemory("/path/to/project")
        engram.remember("We chose JWT because of stateless auth needs", memory_type="decision")
        results = engram.recall("authentication approach")
    """

    def __init__(self, project_path: str | Path, config: EngramConfig = None):
        self.config = config or load_config(project_path)
        self._ensure_dirs()

        # Initialize components
        from .store import MemoryStore
        from .search import MemorySearchIndex
        from .entities import EntityGraph, create_graph_backend

        self.store = MemoryStore(self.config.memories_db_path)
        self.search_index = MemorySearchIndex(use_semantic=self.config.use_semantic)
        self.history = HistoryDB(self.config.history_db_path)

        # Entity graph
        graph_backend = create_graph_backend(
            self.config.graph_config, self.config.graph_db_path
        )
        self.entity_graph = EntityGraph(graph_backend)

        # Build search index from existing memories
        self._load_index()

        logger.info(
            f"Engram initialized: project={self.config.project_name}, "
            f"hash={self.config.project_hash}, "
            f"memories={self.store.count()}, "
            f"semantic={'yes' if self.search_index.has_semantic else 'no'}"
        )

    def _ensure_dirs(self):
        """Create .engram/ directory structure."""
        self.config.engram_dir.mkdir(parents=True, exist_ok=True)
        self.config.export_dir.mkdir(parents=True, exist_ok=True)

    def _load_index(self):
        """Load all memories into the search index (cold start)."""
        user_id = self._scope_to_user_id("project")
        all_records = self.store.get_all(user_id=user_id, limit=10000)

        # Also load session and user scoped memories
        for scope in ("session", "user"):
            uid = self._scope_to_user_id(scope)
            if uid != user_id:
                all_records.extend(self.store.get_all(user_id=uid, limit=10000))

        if all_records:
            self.search_index.build_from_records(all_records)
            logger.info(f"Search index loaded: {len(all_records)} records")

    # ─── Remember ────────────────────────────────────────────────

    def remember(
        self,
        content: str,
        memory_type: str = "fact",
        scope: str = "project",
        metadata: dict = None,
    ) -> dict:
        """
        Store a memory.

        Args:
            content: The memory content (natural language)
            memory_type: fact, decision, pattern, preference, context, trace
            scope: project, session, user
            metadata: Additional metadata to store
        """
        user_id = self._scope_to_user_id(scope)

        record = self.store.add(
            content=content,
            memory_type=memory_type,
            scope=scope,
            user_id=user_id,
            project_name=self.config.project_name,
            project_hash=self.config.project_hash,
            tags=(metadata or {}).pop("tags", []) if metadata else [],
            extra_metadata={
                "created_at": datetime.now(timezone.utc).isoformat(),
                **(metadata or {}),
            },
        )

        # Add to search index
        self.search_index.add_record(record)

        # Extract and index entities
        self.entity_graph.index_memory(record.id, content)

        # Audit trail
        self.history.log_operation(
            operation="remember",
            memory_type=memory_type,
            scope=scope,
            content_preview=content,
            memory_id=record.id,
            metadata={"memory_type": memory_type, "scope": scope},
        )

        logger.info(f"Remembered ({memory_type}/{scope}): {content[:80]}...")
        return {"results": [record.to_dict()]}

    # ─── Recall ──────────────────────────────────────────────────

    def recall(
        self,
        query: str,
        top_k: int = None,
        scopes: list[str] = None,
        memory_types: list[str] = None,
        since: str = "",
        until: str = "",
    ) -> list[MemoryResult]:
        """
        Search memory for relevant context.

        Args:
            query: Natural language search query
            top_k: Number of results (default: max_context_memories from config)
            scopes: Filter to specific scopes (project, session, user)
            memory_types: Filter to specific types (fact, decision, pattern, etc.)
            since: ISO 8601 date/datetime lower bound (inclusive)
            until: ISO 8601 date/datetime upper bound (inclusive)
        """
        top_k = top_k or self.config.max_context_memories

        results = self.search_index.search(
            query,
            top_k=top_k,
            scope_filter=scopes,
            type_filter=memory_types,
            since=since or None,
            until=until or None,
        )

        memory_results = [
            MemoryResult(
                memory=r.record.content,
                memory_id=r.record.id,
                score=r.score,
                metadata={
                    "memory_type": r.record.memory_type,
                    "scope": r.record.scope,
                    "created_at": r.record.created_at,
                    "tags": r.record.tags,
                    **r.record.extra_metadata,
                },
                source="engram",
            )
            for r in results
        ]

        self.history.log_operation(
            operation="recall",
            memory_type="query",
            scope=",".join(scopes or ["project", "session", "user"]),
            content_preview=query,
            metadata={"results_count": len(memory_results)},
        )

        return memory_results

    # ─── History (temporal entity queries) ─────────────────────

    def engram_history(
        self,
        entity: str,
        since: str = "",
        until: str = "",
        limit: int = 20,
    ) -> list[dict]:
        """
        Get temporal history for an entity — how memories about it evolved over time.

        Args:
            entity: Entity name to trace history for
            since: ISO 8601 date/datetime filter (inclusive lower bound)
            until: ISO 8601 date/datetime filter (inclusive upper bound)
            limit: Max results (default: 20)

        Returns:
            List of dicts with memory content, type, and timestamps, sorted oldest-first.
        """
        memory_ids = self.entity_graph.backend.get_entity_memories_in_range(
            entity, since=since, until=until
        )

        if not memory_ids:
            return []

        results = []
        for mid in memory_ids:
            record = self.store.get(mid)
            if record is None:
                continue
            # Apply time filter on actual memory created_at too
            if since and record.created_at < since:
                continue
            if until and record.created_at > until:
                continue
            results.append({
                "memory_id": record.id,
                "content": record.content,
                "memory_type": record.memory_type,
                "scope": record.scope,
                "created_at": record.created_at,
                "tags": record.tags,
            })

        # Sort chronologically (oldest first)
        results.sort(key=lambda r: r["created_at"])
        return results[:limit]

    # ─── Relate ──────────────────────────────────────────────────

    def relate(self, entity: str, max_depth: int = 2) -> dict:
        """
        Explore entity relationships in the graph.

        Returns dict with entity info, connected nodes/edges, and
        related memories.
        """
        graph_data = self.entity_graph.relate(entity, max_depth=max_depth)

        # Enrich with memory content for top related entities
        memories = []
        memory_ids = set()
        for ent in graph_data.get("entities", []):
            ent_memory_ids = self.entity_graph.backend.get_entity_memories(ent["name"])
            for mid in ent_memory_ids[:3]:  # Top 3 per entity
                if mid not in memory_ids:
                    memory_ids.add(mid)
                    record = self.store.get(mid)
                    if record:
                        memories.append({
                            "memory": record.content[:100],
                            "score": 1.0 / (ent.get("depth", 0) + 1),
                            "entity": ent["name"],
                        })

        return {
            "entity": entity,
            "memories": memories[:10],
            "relations": graph_data.get("relations", []),
            "entities": graph_data.get("entities", []),
            "depth": max_depth,
        }

    # ─── Learn from Conversation ─────────────────────────────────

    def learn_from_conversation(
        self,
        messages: list[dict],
        scope: str = "project",
    ) -> dict:
        """
        Store conversation context as memories.

        Args:
            messages: List of {"role": "user"|"assistant", "content": "..."}
            scope: Memory scope for stored memories
        """
        # Store the conversation as a single context memory
        content_parts = []
        for msg in messages:
            role = msg.get("role", "unknown")
            text = msg.get("content", "")[:500]  # Truncate per message
            content_parts.append(f"[{role}] {text}")

        content = "\n".join(content_parts)[:2000]

        result = self.remember(
            content,
            memory_type="context",
            scope=scope,
            metadata={"source": "conversation", "message_count": len(messages)},
        )

        self.history.log_operation(
            operation="learn",
            memory_type="context",
            scope=scope,
            content_preview=f"Extracted from {len(messages)} messages",
            metadata={"message_count": len(messages)},
        )

        return result

    # ─── Trace ───────────────────────────────────────────────────

    def trace(
        self,
        task: str,
        reasoning: list[str | dict],
        outcome: str = "success",
        tools_used: list[str] = None,
        duration_minutes: float = None,
        related_entities: list[str] = None,
    ) -> dict:
        """
        Store a reasoning trace.

        Args:
            task: Description of the task
            reasoning: List of reasoning steps (strings or step dicts)
            outcome: "success", "failure", "partial"
            tools_used: Tools used during the task
            duration_minutes: How long the task took
            related_entities: Entity names involved
        """
        # Store the trace in history DB
        self.history.log_trace(
            task=task,
            reasoning=reasoning,
            outcome=outcome,
            tools_used=tools_used,
            duration_minutes=duration_minutes,
            related_entities=related_entities,
        )

        # Also store a summary in memory store for semantic search
        reasoning_text = "\n".join(
            s if isinstance(s, str) else s.get("content", str(s))
            for s in reasoning
        )
        summary = (
            f"Task: {task}\n"
            f"Outcome: {outcome}\n"
            f"Approach: {reasoning_text}\n"
            f"Tools: {', '.join(tools_used or [])}"
        )

        result = self.remember(
            summary,
            memory_type="trace",
            scope="project",
            metadata={
                "task": task,
                "outcome": outcome,
                "tools_used": tools_used or [],
                "related_entities": related_entities or [],
            },
        )

        logger.info(f"Trace stored: {task} ({outcome})")
        return result

    def recall_traces(self, task_query: str = None, limit: int = 5) -> list[dict]:
        """Recall reasoning traces from history."""
        return self.history.get_traces(task_query=task_query, limit=limit)

    # ─── Bridge (Serena Integration) ─────────────────────────────

    def bridge_recall(
        self,
        query: str,
        serena_root: str | Path = None,
        top_k: int = 5,
    ) -> BridgeResult:
        """
        Combined search across Engram memory AND Serena docs.

        Args:
            query: Natural language query
            serena_root: Project root for Serena (default: same as Engram project)
            top_k: Results per system
        """
        result = BridgeResult()

        # Engram results
        result.engram_results = self.recall(query, top_k=top_k)

        # Graph edges (if available)
        graph_data = self.relate(query)
        result.graph_edges = graph_data.get("relations", [])

        # Serena results (optional, fail gracefully)
        if self.config.serena_enabled and self.config.serena_search_on_recall:
            serena_root = Path(serena_root) if serena_root else self.config.project_root
            try:
                retriever = _get_serena_retriever(serena_root)
                if retriever is not None:
                    serena_hits = retriever.search(query, top_k=top_k)
                    result.serena_results = serena_hits
                else:
                    logger.debug("Serena retriever unavailable — Engram-only results")
            except Exception as e:
                logger.warning(f"Serena search failed: {e}")

        return result

    # ─── Stats ───────────────────────────────────────────────────

    def stats(self) -> dict:
        """Get memory statistics."""
        history_stats = self.history.get_stats()
        user_id = self._scope_to_user_id("project")
        mem_count = self.store.count(user_id=user_id)
        mem_by_type = self.store.count_by_type(user_id=user_id)
        graph_stats = self.entity_graph.get_stats()

        return {
            "project": self.config.project_name,
            "project_hash": self.config.project_hash,
            "memories_stored": mem_count,
            "memories_by_type": mem_by_type,
            "serena_integration": self.config.serena_enabled,
            "history": history_stats,
            "graph": graph_stats,
            "config": {
                "backend": "sqlite (local)",
                "search": "bm25" + (" + semantic" if self.search_index.has_semantic else ""),
                "graph": graph_stats.get("provider", "sqlite"),
            },
        }

    # ─── Helpers ─────────────────────────────────────────────────

    def _scope_to_user_id(self, scope: str) -> str:
        """Convert scope to user_id for store partitioning."""
        if scope == "user":
            return self.config.user_id
        elif scope == "session":
            return f"{self.config.scoped_user_id}:session"
        else:  # project (default)
            return self.config.scoped_user_id

    def recall_formatted(
        self, query: str, top_k: int = 5, bridge: bool = False,
        since: str = "", until: str = "",
    ) -> str:
        """Search and return formatted output for agent consumption."""
        if bridge:
            result = self.bridge_recall(query, top_k=top_k)
            parts = [f"## Memory + Docs: {query}\n"]

            if result.engram_results:
                parts.append("### Memories\n")
                for i, r in enumerate(result.engram_results, 1):
                    parts.append(
                        f"{i}. [{r.memory_type}] {r.memory}\n"
                        f"   Score: {r.score:.3f} | Scope: {r.scope}"
                    )

            if result.graph_edges:
                parts.append("\n### Graph Relationships\n")
                for edge in result.graph_edges[:10]:
                    if isinstance(edge, dict):
                        parts.append(
                            f"- {edge.get('source', '?')} -> "
                            f"{edge.get('relationship', edge.get('relation_type', '?'))} -> "
                            f"{edge.get('target', '?')}"
                        )

            if result.serena_results:
                parts.append("\n### Documentation\n")
                for i, r in enumerate(result.serena_results, 1):
                    parts.append(f"{i}. {r.chunk.context_header}")

            return "\n".join(parts)
        else:
            results = self.recall(query, top_k=top_k, since=since, until=until)
            if not results:
                return f"No memories found for: {query}"

            parts = [f"## Memory Recall: {query}\n"]
            parts.append(f"Results: {len(results)}\n")
            for i, r in enumerate(results, 1):
                parts.append(
                    f"### {i}. [{r.memory_type}] (score: {r.score:.3f})\n"
                    f"{r.memory}\n"
                    f"Scope: {r.scope} | {r.timestamp}\n"
                )
            return "\n".join(parts)
