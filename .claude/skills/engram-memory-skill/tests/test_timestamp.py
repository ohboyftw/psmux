"""
Tests for timestamp indexing feature in Engram.

Tests cover:
1. SQL index creation on created_at
2. MemoryStore.get_all() with since/until filtering
3. MemorySearchIndex.search() with since/until post-filtering
4. EngramMemory.recall() with since/until
5. EngramMemory.engram_history() for temporal entity queries
6. SQLiteGraphBackend.get_entity_memories_in_range()
7. MCP tool signatures (engram_recall since/until, engram_history)
"""

import sqlite3
import tempfile
import time
from datetime import datetime, timezone, timedelta
from pathlib import Path

import pytest

# ─── Fixtures ────────────────────────────────────────────────────


@pytest.fixture
def tmp_dir(tmp_path):
    """Temp directory for test databases."""
    return tmp_path


@pytest.fixture
def memory_store(tmp_dir):
    """Fresh MemoryStore with test DB."""
    from engram_index.store import MemoryStore
    db_path = tmp_dir / "test_memories.db"
    return MemoryStore(db_path)


@pytest.fixture
def search_index():
    """Fresh MemorySearchIndex (BM25-only, no semantic)."""
    from engram_index.search import MemorySearchIndex
    return MemorySearchIndex(use_semantic=False)


@pytest.fixture
def graph_backend(tmp_dir):
    """Fresh SQLiteGraphBackend."""
    from engram_index.entities import SQLiteGraphBackend
    db_path = tmp_dir / "test_graph.db"
    return SQLiteGraphBackend(db_path)


@pytest.fixture
def engram_memory(tmp_dir):
    """Fresh EngramMemory using temp directory as project root."""
    from engram_index.config import EngramConfig
    from engram_index.memory import EngramMemory

    engram_dir = tmp_dir / ".engram"
    engram_dir.mkdir(parents=True, exist_ok=True)

    config = EngramConfig(
        project_root=tmp_dir,
        project_hash="testhash123",
        project_name="test-project",
        engram_dir=engram_dir,
        use_semantic=False,
        serena_enabled=False,
    )
    return EngramMemory(tmp_dir, config=config)


def _add_memory_at(store, content: str, created_at: str, memory_type: str = "fact"):
    """Helper: insert a memory with a specific created_at timestamp."""
    import hashlib, json, uuid
    c_hash = hashlib.sha256(content.strip().encode("utf-8")).hexdigest()
    mem_id = str(uuid.uuid4())
    with store._connect() as conn:
        conn.execute(
            "INSERT INTO memories (id, content, memory_type, scope, user_id, "
            "project_name, project_hash, created_at, tags, extra_metadata, content_hash) "
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            (mem_id, content, memory_type, "project", "test-user",
             "test", "hash", created_at, json.dumps([]), json.dumps({}), c_hash),
        )
    return mem_id


# ═══════════════════════════════════════════════════════════════════
# Step 1: SQL index on created_at
# ═══════════════════════════════════════════════════════════════════


class TestSQLIndexes:
    """Verify SQL indexes are created on timestamp columns."""

    def test_memories_created_at_index_exists(self, memory_store):
        """MemoryStore._init_db() creates idx_memories_created_at."""
        with memory_store._connect() as conn:
            indexes = conn.execute(
                "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='memories'"
            ).fetchall()
            index_names = [r["name"] for r in indexes]
        assert "idx_memories_created_at" in index_names

    def test_history_timestamp_index_exists(self, tmp_dir):
        """HistoryDB._init_db() creates idx_history_timestamp."""
        from engram_index.memory import HistoryDB
        db_path = tmp_dir / "test_history.db"
        history = HistoryDB(db_path)
        with sqlite3.connect(str(db_path)) as conn:
            conn.row_factory = sqlite3.Row
            indexes = conn.execute(
                "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='memory_history'"
            ).fetchall()
            index_names = [r["name"] for r in indexes]
        assert "idx_history_timestamp" in index_names

    def test_traces_timestamp_index_exists(self, tmp_dir):
        """HistoryDB._init_db() creates idx_traces_timestamp."""
        from engram_index.memory import HistoryDB
        db_path = tmp_dir / "test_history.db"
        history = HistoryDB(db_path)
        with sqlite3.connect(str(db_path)) as conn:
            conn.row_factory = sqlite3.Row
            indexes = conn.execute(
                "SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='traces'"
            ).fetchall()
            index_names = [r["name"] for r in indexes]
        assert "idx_traces_timestamp" in index_names


# ═══════════════════════════════════════════════════════════════════
# Step 2: MemoryStore.get_all() with since/until
# ═══════════════════════════════════════════════════════════════════


class TestStoreTimeFiltering:
    """Test since/until params in MemoryStore.get_all()."""

    def test_get_all_no_filters_returns_all(self, memory_store):
        """Baseline: get_all without since/until returns everything."""
        _add_memory_at(memory_store, "old memory", "2026-01-01T00:00:00+00:00")
        _add_memory_at(memory_store, "new memory", "2026-02-15T00:00:00+00:00")

        results = memory_store.get_all()
        assert len(results) == 2

    def test_get_all_since_filters_old(self, memory_store):
        """since parameter excludes memories before the date."""
        _add_memory_at(memory_store, "january memory", "2026-01-01T00:00:00+00:00")
        _add_memory_at(memory_store, "february memory", "2026-02-15T00:00:00+00:00")

        results = memory_store.get_all(since="2026-02-01")
        assert len(results) == 1
        assert results[0].content == "february memory"

    def test_get_all_until_filters_new(self, memory_store):
        """until parameter excludes memories after the date."""
        _add_memory_at(memory_store, "january memory", "2026-01-01T00:00:00+00:00")
        _add_memory_at(memory_store, "february memory", "2026-02-15T00:00:00+00:00")

        results = memory_store.get_all(until="2026-01-31")
        assert len(results) == 1
        assert results[0].content == "january memory"

    def test_get_all_since_and_until_range(self, memory_store):
        """Combined since+until selects a date range."""
        _add_memory_at(memory_store, "dec memory", "2025-12-15T00:00:00+00:00")
        _add_memory_at(memory_store, "jan memory", "2026-01-15T00:00:00+00:00")
        _add_memory_at(memory_store, "feb memory", "2026-02-15T00:00:00+00:00")

        results = memory_store.get_all(since="2026-01-01", until="2026-01-31")
        assert len(results) == 1
        assert results[0].content == "jan memory"

    def test_get_all_since_empty_string_ignored(self, memory_store):
        """Empty string since/until should be treated as no filter."""
        _add_memory_at(memory_store, "memory one", "2026-01-01T00:00:00+00:00")

        results = memory_store.get_all(since="", until="")
        assert len(results) == 1

    def test_get_all_empty_created_at_excluded_by_since(self, memory_store):
        """Memories with empty created_at are excluded when since is set."""
        # Simulate a hypothetical record with empty created_at
        import json, uuid, hashlib
        with memory_store._connect() as conn:
            conn.execute(
                "INSERT INTO memories (id, content, memory_type, scope, user_id, "
                "project_name, project_hash, created_at, tags, extra_metadata, content_hash) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (str(uuid.uuid4()), "legacy memory", "fact", "project", "test-user",
                 "test", "hash", "", json.dumps([]), json.dumps({}),
                 hashlib.sha256(b"legacy memory").hexdigest()),
            )
        _add_memory_at(memory_store, "dated memory", "2026-02-15T00:00:00+00:00")

        # Without filters: both returned
        assert len(memory_store.get_all()) == 2
        # With since: legacy excluded (empty string < any date)
        assert len(memory_store.get_all(since="2025-01-01")) == 1
        assert memory_store.get_all(since="2025-01-01")[0].content == "dated memory"
        # Without since/until: still both
        assert len(memory_store.get_all(since="", until="")) == 2

    def test_get_all_combines_with_existing_filters(self, memory_store):
        """since/until works alongside user_id, scope, memory_type filters."""
        _add_memory_at(memory_store, "old fact", "2026-01-01T00:00:00+00:00", memory_type="fact")
        _add_memory_at(memory_store, "new fact", "2026-02-15T00:00:00+00:00", memory_type="fact")
        _add_memory_at(memory_store, "new decision", "2026-02-15T00:00:00+00:00", memory_type="decision")

        results = memory_store.get_all(memory_type="fact", since="2026-02-01")
        assert len(results) == 1
        assert results[0].content == "new fact"


# ═══════════════════════════════════════════════════════════════════
# Step 3: MemorySearchIndex.search() with since/until
# ═══════════════════════════════════════════════════════════════════


class TestSearchTimeFiltering:
    """Test since/until post-filtering in MemorySearchIndex.search()."""

    def _make_record(self, content: str, created_at: str, memory_type: str = "fact"):
        from engram_index.store import MemoryRecord
        import uuid
        return MemoryRecord(
            id=str(uuid.uuid4()),
            content=content,
            memory_type=memory_type,
            scope="project",
            user_id="test-user",
            created_at=created_at,
        )

    def test_search_without_time_filters_returns_all_matches(self, search_index):
        """Baseline: search without since/until returns all matching records."""
        r1 = self._make_record("python fastapi server", "2026-01-01T00:00:00+00:00")
        r2 = self._make_record("python django server", "2026-02-15T00:00:00+00:00")
        search_index.add_record(r1)
        search_index.add_record(r2)

        results = search_index.search("python server")
        assert len(results) == 2

    def test_search_since_filters_old_results(self, search_index):
        """since parameter filters search results by created_at."""
        r1 = self._make_record("authentication with JWT tokens", "2026-01-01T00:00:00+00:00")
        r2 = self._make_record("authentication with OAuth2 flow", "2026-02-15T00:00:00+00:00")
        search_index.add_record(r1)
        search_index.add_record(r2)

        results = search_index.search("authentication", since="2026-02-01")
        assert len(results) == 1
        assert "OAuth2" in results[0].record.content

    def test_search_until_filters_new_results(self, search_index):
        """until parameter filters search results by created_at."""
        r1 = self._make_record("database migration plan", "2026-01-01T00:00:00+00:00")
        r2 = self._make_record("database optimization plan", "2026-02-15T00:00:00+00:00")
        search_index.add_record(r1)
        search_index.add_record(r2)

        results = search_index.search("database plan", until="2026-01-31")
        assert len(results) == 1
        assert "migration" in results[0].record.content

    def test_search_time_filters_with_scope_filter(self, search_index):
        """Time filters work alongside scope_filter and type_filter."""
        r1 = self._make_record("api endpoint design", "2026-01-01T00:00:00+00:00")
        r2 = self._make_record("api endpoint refactor", "2026-02-15T00:00:00+00:00")
        r1.scope = "project"
        r2.scope = "session"
        search_index.add_record(r1)
        search_index.add_record(r2)

        results = search_index.search(
            "api endpoint", scope_filter=["project"], since="2025-12-01"
        )
        assert len(results) == 1
        assert results[0].record.scope == "project"


# ═══════════════════════════════════════════════════════════════════
# Step 4: EngramMemory.recall() with since/until
# ═══════════════════════════════════════════════════════════════════


class TestRecallTimeFiltering:
    """Test since/until wired through to EngramMemory.recall()."""

    def test_recall_with_since(self, engram_memory):
        """recall() accepts since param and filters results."""
        engram_memory.remember("old architecture decision about microservices", memory_type="decision")
        # Manually backdate one record
        with engram_memory.store._connect() as conn:
            conn.execute(
                "UPDATE memories SET created_at = ? WHERE content LIKE '%microservices%'",
                ("2025-01-01T00:00:00+00:00",),
            )
        # Rebuild the search index to pick up the changed timestamp
        engram_memory._load_index()

        engram_memory.remember("new architecture decision about serverless", memory_type="decision")

        results = engram_memory.recall("architecture decision", since="2026-01-01")
        contents = [r.memory for r in results]
        assert any("serverless" in c for c in contents)
        assert not any("microservices" in c for c in contents)

    def test_recall_with_until(self, engram_memory):
        """recall() accepts until param and filters results."""
        engram_memory.remember("early design choice for auth", memory_type="decision")
        # Manually backdate
        with engram_memory.store._connect() as conn:
            conn.execute(
                "UPDATE memories SET created_at = ? WHERE content LIKE '%early design%'",
                ("2025-06-01T00:00:00+00:00",),
            )
        engram_memory._load_index()

        engram_memory.remember("latest design choice for auth v2", memory_type="decision")

        results = engram_memory.recall("design choice auth", until="2025-12-31")
        contents = [r.memory for r in results]
        assert any("early" in c for c in contents)
        assert not any("latest" in c for c in contents)

    def test_recall_formatted_with_since_until(self, engram_memory):
        """recall_formatted() accepts and passes since/until."""
        engram_memory.remember("cache strategy with Redis", memory_type="decision")
        with engram_memory.store._connect() as conn:
            conn.execute(
                "UPDATE memories SET created_at = ? WHERE content LIKE '%Redis%'",
                ("2025-01-01T00:00:00+00:00",),
            )
        engram_memory._load_index()

        engram_memory.remember("cache strategy with Memcached", memory_type="decision")

        output = engram_memory.recall_formatted("cache strategy", since="2026-01-01")
        assert "Memcached" in output
        assert "Redis" not in output


# ═══════════════════════════════════════════════════════════════════
# Step 5 & 7: engram_history and entity temporal queries
# ═══════════════════════════════════════════════════════════════════


class TestEntityTemporalQueries:
    """Test get_entity_memories_in_range on SQLiteGraphBackend."""

    def test_get_entity_memories_in_range_no_filters(self, graph_backend, memory_store):
        """Without time filters, returns all memories for entity."""
        # Add entity-memory links with timestamps
        from datetime import datetime, timezone
        graph_backend.add_entity("Redis", "mem-1")
        graph_backend.add_entity("Redis", "mem-2")

        result = graph_backend.get_entity_memories_in_range("Redis")
        assert set(result) == {"mem-1", "mem-2"}

    def test_get_entity_memories_in_range_since(self, graph_backend):
        """since filters entity memories by created_at."""
        # We need to manipulate the created_at on the entities table
        with graph_backend._connect() as conn:
            conn.execute(
                "INSERT OR IGNORE INTO entities (name, memory_id, created_at) VALUES (?, ?, ?)",
                ("Redis", "mem-old", "2025-06-01T00:00:00+00:00"),
            )
            conn.execute(
                "INSERT OR IGNORE INTO entities (name, memory_id, created_at) VALUES (?, ?, ?)",
                ("Redis", "mem-new", "2026-02-15T00:00:00+00:00"),
            )

        result = graph_backend.get_entity_memories_in_range("Redis", since="2026-01-01")
        assert result == ["mem-new"]

    def test_get_entity_memories_in_range_until(self, graph_backend):
        """until filters entity memories by created_at."""
        with graph_backend._connect() as conn:
            conn.execute(
                "INSERT OR IGNORE INTO entities (name, memory_id, created_at) VALUES (?, ?, ?)",
                ("JWT", "mem-old", "2025-06-01T00:00:00+00:00"),
            )
            conn.execute(
                "INSERT OR IGNORE INTO entities (name, memory_id, created_at) VALUES (?, ?, ?)",
                ("JWT", "mem-new", "2026-02-15T00:00:00+00:00"),
            )

        result = graph_backend.get_entity_memories_in_range("JWT", until="2025-12-31")
        assert result == ["mem-old"]

    def test_get_entity_memories_in_range_both(self, graph_backend):
        """Combined since+until narrows to a date range."""
        with graph_backend._connect() as conn:
            for ts, mid in [
                ("2025-01-01T00:00:00+00:00", "mem-1"),
                ("2025-06-15T00:00:00+00:00", "mem-2"),
                ("2026-02-15T00:00:00+00:00", "mem-3"),
            ]:
                conn.execute(
                    "INSERT OR IGNORE INTO entities (name, memory_id, created_at) VALUES (?, ?, ?)",
                    ("FastAPI", mid, ts),
                )

        result = graph_backend.get_entity_memories_in_range(
            "FastAPI", since="2025-03-01", until="2025-12-31"
        )
        assert result == ["mem-2"]


class TestEngramHistory:
    """Test EngramMemory.engram_history() method."""

    def test_engram_history_returns_chronological_memories(self, engram_memory):
        """engram_history returns memories for an entity sorted oldest-first."""
        # Store memories that mention extractable entity "RedisCache"
        engram_memory.remember("We chose RedisCache for caching layer", memory_type="decision")
        engram_memory.remember("RedisCache cluster setup completed", memory_type="fact")

        result = engram_memory.engram_history("RedisCache")
        assert isinstance(result, list)
        assert len(result) >= 1
        # Should be chronological (oldest first)
        if len(result) >= 2:
            assert result[0]["created_at"] <= result[1]["created_at"]

    def test_engram_history_with_since(self, engram_memory):
        """engram_history respects since parameter."""
        engram_memory.remember("Old RedisCache config decision", memory_type="decision")
        with engram_memory.store._connect() as conn:
            conn.execute(
                "UPDATE memories SET created_at = ? WHERE content LIKE '%Old RedisCache%'",
                ("2025-01-01T00:00:00+00:00",),
            )
        # Also backdate the entity entry
        with engram_memory.entity_graph.backend._connect() as conn:
            conn.execute(
                "UPDATE entities SET created_at = ? WHERE memory_id IN "
                "(SELECT id FROM (SELECT id FROM entities WHERE name = 'RedisCache' "
                "AND created_at > '2026-01-01') sub)",
                ("2025-01-01T00:00:00+00:00",),
            )
            # Simpler: just backdate all RedisCache entities then re-add new one
            conn.execute(
                "UPDATE entities SET created_at = '2025-01-01T00:00:00+00:00' WHERE name = 'RedisCache'"
            )
        engram_memory._load_index()

        engram_memory.remember("New RedisCache scaling plan", memory_type="fact")

        result = engram_memory.engram_history("RedisCache", since="2026-01-01")
        contents = [r["content"] for r in result]
        assert any("scaling" in c for c in contents)
        assert not any("Old" in c for c in contents)

    def test_engram_history_with_limit(self, engram_memory):
        """engram_history respects limit parameter."""
        for i in range(5):
            engram_memory.remember(f"Memory about DockerCompose instance {i}", memory_type="fact")

        result = engram_memory.engram_history("DockerCompose", limit=2)
        assert len(result) <= 2

    def test_engram_history_empty_entity(self, engram_memory):
        """engram_history for unknown entity returns empty list."""
        result = engram_memory.engram_history("NonExistentEntity12345")
        assert result == []


# ═══════════════════════════════════════════════════════════════════
# Step 6: MCP tool signatures
# ═══════════════════════════════════════════════════════════════════


class TestMCPToolSignatures:
    """Verify MCP tools have the right signatures (import-time checks)."""

    def test_engram_recall_accepts_since_until(self):
        """engram_recall MCP tool should accept since and until params."""
        import inspect
        from mcp_server import engram_recall
        sig = inspect.signature(engram_recall)
        assert "since" in sig.parameters
        assert "until" in sig.parameters

    def test_engram_history_tool_exists(self):
        """engram_history MCP tool should exist."""
        from mcp_server import engram_history
        import inspect
        sig = inspect.signature(engram_history)
        assert "entity" in sig.parameters
        assert "since" in sig.parameters
        assert "until" in sig.parameters
        assert "limit" in sig.parameters
