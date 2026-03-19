#!/usr/bin/env python3
"""Engram HTTP Server — REST API wrapper for OpenClaw agents on Docker network."""

from __future__ import annotations

import logging
import os
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field

from engram_index.memory import EngramMemory

logging.basicConfig(level=logging.INFO, format="%(name)s: %(message)s")
logger = logging.getLogger("engram.http")

app = FastAPI(title="Engram Memory", version="1.0.0")

_memories: dict[str, EngramMemory] = {}


def _get_memory(agent: str) -> EngramMemory:
    """Get or create per-agent EngramMemory instance."""
    if agent not in _memories:
        data_dir = Path(os.environ.get("ENGRAM_DATA_DIR", "/data"))
        agent_dir = data_dir / agent
        agent_dir.mkdir(parents=True, exist_ok=True)
        logger.info(f"Initializing Engram for agent: {agent} at {agent_dir}")
        _memories[agent] = EngramMemory(agent_dir)
    return _memories[agent]


# ─── Request/Response Models ─────────────────────────────────────


class RememberRequest(BaseModel):
    content: str
    memory_type: str = "fact"
    scope: str = "project"


class RecallRequest(BaseModel):
    query: str
    top_k: int = 5
    since: str = ""
    until: str = ""


class RelateRequest(BaseModel):
    entity: str
    max_depth: int = 2


class TraceRequest(BaseModel):
    task: str
    steps: list[str]
    outcome: str = "success"
    tools_used: list[str] = Field(default_factory=list)


# ─── Endpoints ───────────────────────────────────────────────────


@app.post("/api/{agent}/remember")
def remember(agent: str, req: RememberRequest):
    mem = _get_memory(agent)
    result = mem.remember(req.content, memory_type=req.memory_type, scope=req.scope)
    stored = len(result.get("results", [])) if isinstance(result, dict) else 0
    return {"status": "ok", "stored": stored, "content": req.content[:120]}


@app.post("/api/{agent}/recall")
def recall(agent: str, req: RecallRequest):
    mem = _get_memory(agent)
    formatted = mem.recall_formatted(
        req.query, top_k=req.top_k, since=req.since, until=req.until
    )
    return {"status": "ok", "results": formatted}


@app.post("/api/{agent}/relate")
def relate(agent: str, req: RelateRequest):
    mem = _get_memory(agent)
    result = mem.relate(req.entity, max_depth=req.max_depth)
    return {"status": "ok", **result}


@app.post("/api/{agent}/trace")
def trace(agent: str, req: TraceRequest):
    mem = _get_memory(agent)
    mem.trace(
        task=req.task,
        reasoning=req.steps,
        outcome=req.outcome,
        tools_used=req.tools_used,
    )
    return {"status": "ok", "task": req.task, "steps": len(req.steps)}


@app.get("/api/{agent}/stats")
def stats(agent: str):
    mem = _get_memory(agent)
    return mem.stats()


@app.get("/health")
def health():
    return {"status": "ok", "agents": list(_memories.keys())}


if __name__ == "__main__":
    import uvicorn

    port = int(os.environ.get("ENGRAM_PORT", "8100"))
    uvicorn.run(app, host="0.0.0.0", port=port, log_level="info")
