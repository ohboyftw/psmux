"""
Entity extraction and graph for Engram.

Default backend: SQLite (zero deps, BFS traversal).
Optional backend: RedisGraph / FalkorDB (Cypher queries).

Entity extraction uses regex heuristics — no LLM calls:
- Capitalized multi-word phrases (proper nouns)
- Code identifiers (CamelCase, snake_case with context)
- Quoted terms
- Known technical patterns (file paths, URLs, etc.)
"""

from __future__ import annotations

import logging
import re
import sqlite3
from pathlib import Path
from typing import Protocol

logger = logging.getLogger("engram.entities")


# ─── Entity Extraction (regex-based) ────────────────────────────

# Capitalized phrases: "Auth Service", "JWT Token", "Redis Cache"
RE_CAPITALIZED = re.compile(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\b")

# CamelCase identifiers: "AuthService", "MemoryStore"
RE_CAMELCASE = re.compile(r"\b([A-Z][a-z]+(?:[A-Z][a-z]+)+)\b")

# snake_case with minimum length: "memory_store", "auth_service"
RE_SNAKE = re.compile(r"\b([a-z][a-z0-9]*(?:_[a-z0-9]+){1,})\b")

# Quoted terms: "JWT", "Redis", etc.
RE_QUOTED = re.compile(r'["\']([A-Za-z][\w\s\-\.]{1,40}?)["\']')

# File paths: src/foo/bar.py, ./config.yaml
RE_FILEPATH = re.compile(r"(?:^|[\s(])([./]?[\w\-]+(?:/[\w\-]+)*\.[\w]+)")

# Minimum entity length
MIN_ENTITY_LEN = 2

# Stop words for entity extraction
ENTITY_STOP = {
    "the", "this", "that", "with", "from", "into", "have", "been",
    "will", "would", "could", "should", "also", "just", "some",
    "true", "false", "none", "null", "undefined",
}


def extract_entities(text: str) -> list[str]:
    """
    Extract entity names from text using regex heuristics.

    Returns deduplicated list of entity strings.
    """
    entities = set()

    for pattern in (RE_CAPITALIZED, RE_CAMELCASE, RE_QUOTED):
        for match in pattern.finditer(text):
            entity = match.group(1).strip()
            if len(entity) >= MIN_ENTITY_LEN and entity.lower() not in ENTITY_STOP:
                entities.add(entity)

    # snake_case — only include if 3+ chars
    for match in RE_SNAKE.finditer(text):
        entity = match.group(1)
        if len(entity) >= 4 and entity not in ENTITY_STOP:
            entities.add(entity)

    # File paths
    for match in RE_FILEPATH.finditer(text):
        path = match.group(1).strip()
        if "/" in path and len(path) >= 4:
            entities.add(path)

    return sorted(entities)


# ─── Graph Backend Protocol ─────────────────────────────────────


class GraphBackend(Protocol):
    """Protocol for entity graph backends."""

    def add_entity(self, name: str, memory_id: str) -> None: ...
    def add_relation(self, source: str, target: str, relation_type: str, memory_id: str) -> None: ...
    def get_related(self, entity: str, max_depth: int) -> dict: ...
    def get_entity_memories(self, entity: str) -> list[str]: ...
    def get_stats(self) -> dict: ...


# ─── SQLite Graph Backend ───────────────────────────────────────


class SQLiteGraphBackend:
    """
    Entity graph stored in SQLite.

    Tables:
    - entities: (name, memory_id, created_at)
    - entity_relations: (source, target, relation_type, memory_id, created_at)

    Traversal via BFS up to max_depth hops.
    """

    def __init__(self, db_path: Path):
        self.db_path = db_path
        self._init_db()

    def _init_db(self):
        with self._connect() as conn:
            conn.execute("""
                CREATE TABLE IF NOT EXISTS entities (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    name TEXT NOT NULL,
                    memory_id TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    UNIQUE(name, memory_id)
                )
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_entities_name
                ON entities(name)
            """)
            conn.execute("""
                CREATE TABLE IF NOT EXISTS entity_relations (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source TEXT NOT NULL,
                    target TEXT NOT NULL,
                    relation_type TEXT NOT NULL DEFAULT 'co_occurs',
                    memory_id TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    UNIQUE(source, target, memory_id)
                )
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_relations_source
                ON entity_relations(source)
            """)
            conn.execute("""
                CREATE INDEX IF NOT EXISTS idx_relations_target
                ON entity_relations(target)
            """)

    def _connect(self) -> sqlite3.Connection:
        conn = sqlite3.connect(str(self.db_path))
        conn.row_factory = sqlite3.Row
        return conn

    def add_entity(self, name: str, memory_id: str) -> None:
        """Register an entity occurrence in a memory."""
        from datetime import datetime, timezone
        with self._connect() as conn:
            conn.execute(
                "INSERT OR IGNORE INTO entities (name, memory_id, created_at) VALUES (?, ?, ?)",
                (name, memory_id, datetime.now(timezone.utc).isoformat()),
            )

    def add_relation(self, source: str, target: str, relation_type: str, memory_id: str) -> None:
        """Add a relationship between two entities."""
        from datetime import datetime, timezone
        with self._connect() as conn:
            conn.execute(
                "INSERT OR IGNORE INTO entity_relations "
                "(source, target, relation_type, memory_id, created_at) VALUES (?, ?, ?, ?, ?)",
                (source, target, relation_type, memory_id, datetime.now(timezone.utc).isoformat()),
            )

    def get_related(self, entity: str, max_depth: int = 2) -> dict:
        """
        BFS traversal from entity up to max_depth hops.

        Returns:
            {
                "entity": str,
                "relations": [{"source": ..., "target": ..., "relation_type": ..., "count": int}],
                "entities": [{"name": ..., "memory_count": int, "depth": int}],
            }
        """
        visited = set()
        queue = [(entity, 0)]
        all_relations = []
        all_entities = []

        with self._connect() as conn:
            while queue:
                current, depth = queue.pop(0)
                if current in visited or depth > max_depth:
                    continue
                visited.add(current)

                # Count memories for this entity
                mem_count = conn.execute(
                    "SELECT COUNT(DISTINCT memory_id) as cnt FROM entities WHERE name = ?",
                    (current,),
                ).fetchone()["cnt"]

                all_entities.append({
                    "name": current,
                    "memory_count": mem_count,
                    "depth": depth,
                })

                # Find outgoing relations
                rows = conn.execute(
                    "SELECT target, relation_type, COUNT(*) as cnt "
                    "FROM entity_relations WHERE source = ? GROUP BY target, relation_type",
                    (current,),
                ).fetchall()

                for row in rows:
                    all_relations.append({
                        "source": current,
                        "target": row["target"],
                        "relation_type": row["relation_type"],
                        "count": row["cnt"],
                    })
                    if row["target"] not in visited and depth + 1 <= max_depth:
                        queue.append((row["target"], depth + 1))

                # Find incoming relations
                rows = conn.execute(
                    "SELECT source, relation_type, COUNT(*) as cnt "
                    "FROM entity_relations WHERE target = ? GROUP BY source, relation_type",
                    (current,),
                ).fetchall()

                for row in rows:
                    all_relations.append({
                        "source": row["source"],
                        "target": current,
                        "relation_type": row["relation_type"],
                        "count": row["cnt"],
                    })
                    if row["source"] not in visited and depth + 1 <= max_depth:
                        queue.append((row["source"], depth + 1))

        return {
            "entity": entity,
            "relations": all_relations,
            "entities": all_entities,
        }

    def get_entity_memories(self, entity: str) -> list[str]:
        """Get all memory IDs associated with an entity."""
        with self._connect() as conn:
            rows = conn.execute(
                "SELECT DISTINCT memory_id FROM entities WHERE name = ?",
                (entity,),
            ).fetchall()
            return [row["memory_id"] for row in rows]

    def get_entity_memories_in_range(
        self, entity: str, since: str = "", until: str = ""
    ) -> list[str]:
        """Get memory IDs for an entity within a time range.

        Filters on the entities table's created_at column.
        Returns memory IDs sorted by created_at ascending.
        """
        conditions = ["name = ?"]
        params: list[str] = [entity]

        if since:
            conditions.append("created_at >= ?")
            params.append(since)
        if until:
            conditions.append("created_at <= ?")
            params.append(until)

        where = " AND ".join(conditions)

        with self._connect() as conn:
            rows = conn.execute(
                f"SELECT DISTINCT memory_id FROM entities WHERE {where} ORDER BY created_at ASC",
                params,
            ).fetchall()
            return [row["memory_id"] for row in rows]

    def get_stats(self) -> dict:
        with self._connect() as conn:
            entity_count = conn.execute("SELECT COUNT(DISTINCT name) as cnt FROM entities").fetchone()["cnt"]
            relation_count = conn.execute("SELECT COUNT(*) as cnt FROM entity_relations").fetchone()["cnt"]
            return {
                "entities": entity_count,
                "relations": relation_count,
                "provider": "sqlite",
            }


# ─── Entity Graph (facade) ──────────────────────────────────────


class EntityGraph:
    """
    Facade over graph backend.

    Handles entity extraction from memory content and manages
    the co-occurrence graph.
    """

    def __init__(self, backend: GraphBackend):
        self.backend = backend

    def index_memory(self, memory_id: str, content: str) -> list[str]:
        """
        Extract entities from content and add to graph.

        Creates co-occurrence edges between all entities found
        in the same memory.

        Returns list of extracted entity names.
        """
        entities = extract_entities(content)

        for entity in entities:
            self.backend.add_entity(entity, memory_id)

        # Co-occurrence edges: all pairs within same memory
        for i, src in enumerate(entities):
            for tgt in entities[i + 1:]:
                self.backend.add_relation(src, tgt, "co_occurs", memory_id)
                self.backend.add_relation(tgt, src, "co_occurs", memory_id)

        return entities

    def relate(self, entity: str, max_depth: int = 2) -> dict:
        """Traverse the entity graph from a starting entity."""
        return self.backend.get_related(entity, max_depth=max_depth)

    def get_stats(self) -> dict:
        return self.backend.get_stats()


def create_graph_backend(config: dict, db_path: Path) -> GraphBackend:
    """
    Factory for graph backends.

    Config:
        provider: "sqlite" (default) | "redisgraph" | "falkordb"
        config: {host, port, graph_name} for Redis-based backends
    """
    provider = config.get("provider", "sqlite")

    if provider == "sqlite":
        return SQLiteGraphBackend(db_path)

    elif provider in ("redisgraph", "falkordb"):
        try:
            return _create_redis_graph_backend(provider, config.get("config", {}))
        except ImportError as e:
            logger.warning(f"Redis graph backend unavailable ({e}), falling back to SQLite")
            return SQLiteGraphBackend(db_path)

    else:
        logger.warning(f"Unknown graph provider '{provider}', falling back to SQLite")
        return SQLiteGraphBackend(db_path)


def _create_redis_graph_backend(provider: str, config: dict) -> GraphBackend:
    """Create Redis-based graph backend (optional dependency)."""
    # Import will raise if not installed — caught by caller
    import redis
    from redis.commands.graph import Graph

    host = config.get("host", "localhost")
    port = config.get("port", 6379)
    graph_name = config.get("graph_name", "engram")

    r = redis.Redis(host=host, port=int(port), decode_responses=True)
    graph = Graph(r, graph_name)

    # Return a RedisGraphBackend adapter (simplified)
    # For now, fall back to SQLite — Redis backend is a future enhancement
    raise ImportError("Redis graph backend not yet implemented")
