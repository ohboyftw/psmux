"""
Configuration loading and validation for RLM Workspace.

Supports:
- .rlm/config.yaml in project root
- Environment variable overrides (${VAR} syntax in YAML)
- Sensible defaults for zero-config startup with local Ollama
"""

from __future__ import annotations

import os
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml


# --- Defaults ---------------------------------------------------------------

DEFAULT_CONFIG: dict[str, Any] = {
    "backend": "openai",
    "backend_kwargs": {
        "model_name": "qwen3:8b",
        "base_url": "http://localhost:11434/v1",
        "api_key": "ollama",
    },
    "environment": "local",
    "max_iterations": 30,
    "verbose": False,
}


# --- Environment Variable Expansion -----------------------------------------

ENV_VAR_RE = re.compile(r"\$\{([^}:]+)(?::-(.*?))?\}")


def expand_env(value: Any) -> Any:
    """Recursively expand ${VAR:-default} in config values."""
    if isinstance(value, str):
        def replacer(match: re.Match) -> str:
            var_name = match.group(1)
            default = match.group(2) or ""
            return os.environ.get(var_name, default)
        return ENV_VAR_RE.sub(replacer, value)
    elif isinstance(value, dict):
        return {k: expand_env(v) for k, v in value.items()}
    elif isinstance(value, list):
        return [expand_env(v) for v in value]
    return value


# --- Project Detection -------------------------------------------------------

def detect_project_root(start: Path) -> Path:
    """Walk up to find project root (directory with .git, package.json, etc.)."""
    markers = [".git", "package.json", "pyproject.toml", "Cargo.toml", "go.mod"]
    current = start.resolve()
    while current != current.parent:
        for marker in markers:
            if (current / marker).exists():
                return current
        current = current.parent
    return start.resolve()


# --- Config Dataclass --------------------------------------------------------

@dataclass
class RLMConfig:
    """Validated configuration for RLM Workspace."""

    project_root: Path
    rlm_dir: Path

    # RLM constructor args (used for autonomous mode)
    backend: str = "openai"
    backend_kwargs: dict[str, Any] = field(default_factory=lambda: {
        "model_name": "qwen3:8b",
        "base_url": "http://localhost:11434/v1",
        "api_key": "ollama",
    })
    environment: str = "local"
    max_iterations: int = 30
    verbose: bool = False

    # REPL settings (used for agent-driven mode)
    repl_timeout: float = 30.0


def load_config(project_path: str | Path) -> RLMConfig:
    """
    Load RLM Workspace configuration.

    Priority: .rlm/config.yaml > environment variables > defaults
    """
    root = detect_project_root(Path(project_path))
    rlm_dir = root / ".rlm"
    config_file = rlm_dir / "config.yaml"

    # Start with defaults
    raw: dict[str, Any] = dict(DEFAULT_CONFIG)
    # Deep-copy backend_kwargs so mutations don't affect DEFAULT_CONFIG
    raw["backend_kwargs"] = dict(DEFAULT_CONFIG["backend_kwargs"])

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

    return RLMConfig(
        project_root=root,
        rlm_dir=rlm_dir,
        backend=raw.get("backend", "openai"),
        backend_kwargs=raw.get("backend_kwargs", {}),
        environment=raw.get("environment", "local"),
        max_iterations=int(raw.get("max_iterations", 30)),
        verbose=bool(raw.get("verbose", False)),
        repl_timeout=float(raw.get("repl_timeout", 30.0)),
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
