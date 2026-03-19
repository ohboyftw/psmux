"""
Configuration loading and validation for Engram.

Supports:
- .engram/config.yaml in project root
- Environment variable overrides (${VAR} syntax in YAML)
- Sensible defaults for zero-config startup
"""

from __future__ import annotations

import hashlib
import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Optional

import yaml


# ─── Defaults ────────────────────────────────────────────────────

DEFAULT_CONFIG = {
    "version": "2.0",
    "search": {
        "use_semantic": True,
    },
    "graph": {
        "provider": "sqlite",
    },
    "auto_extract": False,
    "auto_extract_interval": 5,
    "max_context_memories": 10,
    "memory_ttl_days": 365,
    "consolidation_enabled": True,
    "user_id": "default",
    "project_scope": True,
    "session_scope": True,
    "serena": {
        "enabled": "auto",
        "search_on_recall": True,
    },
}


# ─── Environment Variable Expansion ─────────────────────────────

ENV_VAR_RE = re.compile(r"\$\{([^}:]+)(?::-(.*?))?\}")


def expand_env(value: Any) -> Any:
    """Recursively expand ${VAR:-default} in config values."""
    if isinstance(value, str):
        def replacer(match):
            var_name = match.group(1)
            default = match.group(2) or ""
            return os.environ.get(var_name, default)
        return ENV_VAR_RE.sub(replacer, value)
    elif isinstance(value, dict):
        return {k: expand_env(v) for k, v in value.items()}
    elif isinstance(value, list):
        return [expand_env(v) for v in value]
    return value


# ─── Project Detection ───────────────────────────────────────────


def detect_project_root(start: Path) -> Path:
    """Walk up to find project root (directory with .git, package.json, etc.)."""
    markers = [".engram", ".git", "package.json", "pyproject.toml", "Cargo.toml", "go.mod"]
    current = start.resolve()
    while current != current.parent:
        for marker in markers:
            if (current / marker).exists():
                return current
        current = current.parent
    return start.resolve()  # Fallback to start directory


def compute_project_hash(root: Path) -> str:
    """SHA256 hash of the absolute path — used for memory scoping."""
    return hashlib.sha256(str(root.resolve()).encode()).hexdigest()[:16]


def detect_project_name(root: Path) -> str:
    """Try to detect project name from package files."""
    # pyproject.toml
    pyproject = root / "pyproject.toml"
    if pyproject.exists():
        try:
            text = pyproject.read_text()
            for line in text.splitlines():
                if line.strip().startswith("name"):
                    name = line.split("=", 1)[1].strip().strip('"').strip("'")
                    if name:
                        return name
        except Exception:
            pass

    # package.json
    pkg = root / "package.json"
    if pkg.exists():
        try:
            import json
            data = json.loads(pkg.read_text())
            if "name" in data:
                return data["name"]
        except Exception:
            pass

    # Cargo.toml
    cargo = root / "Cargo.toml"
    if cargo.exists():
        try:
            text = cargo.read_text()
            for line in text.splitlines():
                if line.strip().startswith("name"):
                    name = line.split("=", 1)[1].strip().strip('"')
                    if name:
                        return name
        except Exception:
            pass

    # Fallback: directory name
    return root.name


# ─── Config Dataclass ────────────────────────────────────────────


@dataclass
class EngramConfig:
    """Validated configuration for Engram."""

    project_root: Path
    project_hash: str
    project_name: str
    engram_dir: Path

    # Search config
    use_semantic: bool = True

    # Graph config
    graph_config: dict = field(default_factory=lambda: {"provider": "sqlite"})

    # Behavior
    auto_extract: bool = False
    auto_extract_interval: int = 5
    max_context_memories: int = 10
    memory_ttl_days: int = 365
    consolidation_enabled: bool = True

    # Scoping
    user_id: str = "default"
    project_scope: bool = True
    session_scope: bool = True

    # Serena integration
    serena_enabled: bool = False
    serena_search_on_recall: bool = True
    serena_dir: Optional[Path] = None

    @property
    def history_db_path(self) -> Path:
        return self.engram_dir / "history.db"

    @property
    def memories_db_path(self) -> Path:
        return self.engram_dir / "memories.db"

    @property
    def graph_db_path(self) -> Path:
        return self.engram_dir / "graph.db"

    @property
    def export_dir(self) -> Path:
        return self.engram_dir / "export"

    @property
    def scoped_user_id(self) -> str:
        """User ID scoped to this project."""
        if self.project_scope:
            return f"{self.user_id}:{self.project_hash}"
        return self.user_id


def load_config(project_path: str | Path) -> EngramConfig:
    """
    Load Engram configuration.

    Priority: .engram/config.yaml > environment variables > defaults
    """
    root = detect_project_root(Path(project_path))
    engram_dir = root / ".engram"
    config_file = engram_dir / "config.yaml"

    # Start with defaults
    raw = dict(DEFAULT_CONFIG)

    # Overlay config file if it exists
    if config_file.exists():
        try:
            with open(config_file) as f:
                file_config = yaml.safe_load(f) or {}
            raw = _deep_merge(raw, file_config)
        except Exception as e:
            print(f"Warning: Could not load {config_file}: {e}")

    # Expand environment variables
    raw = expand_env(raw)

    # Detect project metadata
    project_hash = raw.get("project_hash") or compute_project_hash(root)
    project_name = raw.get("project_name") or detect_project_name(root)

    # Search config
    search_config = raw.get("search", {})
    use_semantic = search_config.get("use_semantic", True)

    # Graph config
    graph_config = raw.get("graph", {"provider": "sqlite"})

    # Detect Serena
    serena_setting = raw.get("serena", {}).get("enabled", "auto")
    serena_dir = root / ".serena"
    if serena_setting == "auto":
        serena_enabled = serena_dir.exists()
    elif serena_setting in (True, "true", "yes"):
        serena_enabled = True
    else:
        serena_enabled = False

    return EngramConfig(
        project_root=root,
        project_hash=project_hash,
        project_name=project_name,
        engram_dir=engram_dir,
        use_semantic=use_semantic,
        graph_config=graph_config,
        auto_extract=raw.get("auto_extract", False),
        auto_extract_interval=raw.get("auto_extract_interval", 5),
        max_context_memories=raw.get("max_context_memories", 10),
        memory_ttl_days=raw.get("memory_ttl_days", 365),
        consolidation_enabled=raw.get("consolidation_enabled", True),
        user_id=raw.get("user_id", "default"),
        project_scope=raw.get("project_scope", True),
        session_scope=raw.get("session_scope", True),
        serena_enabled=serena_enabled,
        serena_search_on_recall=raw.get("serena", {}).get("search_on_recall", True),
        serena_dir=serena_dir if serena_enabled else None,
    )


def _deep_merge(base: dict, overlay: dict) -> dict:
    """Recursively merge overlay into base."""
    result = dict(base)
    for key, value in overlay.items():
        if key in result and isinstance(result[key], dict) and isinstance(value, dict):
            result[key] = _deep_merge(result[key], value)
        else:
            result[key] = value
    return result
