"""
Stateful workspace manager for the RLM MCP server.

Manages the sandboxed REPL, file loading, content tracking, and
optional delegation to the rlm library for autonomous analysis.
"""

from __future__ import annotations

import fnmatch
from pathlib import Path
from typing import Any

from .config import RLMConfig, load_config
from .repl import SandboxedREPL


class Workspace:
    """
    Stateful workspace combining file loading, REPL execution,
    and optional autonomous RLM analysis.
    """

    def __init__(self, config: RLMConfig | None = None):
        self.config = config or load_config(Path.cwd())
        self.repl = SandboxedREPL(timeout=self.config.repl_timeout)
        self._loaded_files: dict[str, dict[str, Any]] = {}  # var_name -> metadata

    def load(
        self,
        path: str,
        var_name: str = "context",
        recursive: bool = False,
        glob_pattern: str = "",
    ) -> dict[str, Any]:
        """
        Load file(s) into the REPL as a Python variable.

        Returns metadata about what was loaded.
        """
        target = Path(path).resolve()

        if target.is_file():
            content = target.read_text(encoding="utf-8", errors="replace")
            self.repl.set_var(var_name, content)
            meta = {
                "var_name": var_name,
                "files": 1,
                "total_chars": len(content),
                "total_lines": content.count("\n") + 1,
                "paths": [str(target)],
            }
            self._loaded_files[var_name] = meta
            return meta

        if not target.is_dir():
            raise FileNotFoundError(f"Path not found: {path}")

        # Collect files from directory
        files: list[Path] = []
        if recursive:
            candidates = target.rglob("*")
        else:
            candidates = target.iterdir()

        for p in candidates:
            if not p.is_file():
                continue
            if glob_pattern and not fnmatch.fnmatch(p.name, glob_pattern):
                continue
            # Skip binary-looking files
            if p.suffix in {".pyc", ".pyo", ".so", ".dll", ".exe", ".bin", ".db",
                            ".sqlite", ".jpg", ".png", ".gif", ".ico", ".woff",
                            ".woff2", ".ttf", ".eot", ".zip", ".tar", ".gz"}:
                continue
            files.append(p)

        if not files:
            return {"var_name": var_name, "files": 0, "total_chars": 0, "error": "No matching files found"}

        files.sort()

        # Concatenate with file markers
        parts: list[str] = []
        loaded_paths: list[str] = []
        total_chars = 0
        for f in files:
            try:
                text = f.read_text(encoding="utf-8", errors="replace")
                rel = f.relative_to(target)
                parts.append(f"# === {rel} ===\n{text}")
                loaded_paths.append(str(rel))
                total_chars += len(text)
            except (OSError, UnicodeDecodeError):
                continue

        combined = "\n\n".join(parts)
        self.repl.set_var(var_name, combined)

        meta = {
            "var_name": var_name,
            "files": len(loaded_paths),
            "total_chars": total_chars,
            "total_lines": combined.count("\n") + 1,
            "paths": loaded_paths[:50],  # cap the list for display
        }
        if len(loaded_paths) > 50:
            meta["note"] = f"Showing first 50 of {len(loaded_paths)} files"

        self._loaded_files[var_name] = meta
        return meta

    def execute(self, code: str) -> str:
        """Execute code in the REPL and return formatted output."""
        result = self.repl.execute(code)
        output_parts: list[str] = []
        if result["stdout"]:
            output_parts.append(result["stdout"].rstrip())
        if result["error"]:
            output_parts.append(f"[ERROR] {result['error']}")
        return "\n".join(output_parts) if output_parts else "(no output)"

    def get_vars(self) -> str:
        """List all workspace variables with metadata."""
        repl_vars = self.repl.list_vars()
        if not repl_vars:
            return "No variables loaded."

        lines: list[str] = []
        for name, info in sorted(repl_vars.items()):
            parts = [f"  {name}: {info['type']}"]
            if "size" in info:
                parts.append(f"({info['size']:,} chars, {info['lines']:,} lines)")
            elif "length" in info:
                parts.append(f"({info['length']:,} items)")
            lines.append(" ".join(parts))

        return "\n".join(lines)

    def analyze(self, path: str, question: str, max_iterations: int = 30) -> str:
        """
        Run autonomous RLM analysis using the rlm library.

        Requires the `rlms` package to be installed. Falls back with
        a clear error message if not available.
        """
        try:
            from rlm import RLM
        except ImportError:
            return (
                "[ERROR] The 'rlms' package is not installed. "
                "Autonomous analysis requires it.\n"
                "Install with: pip install rlms\n\n"
                "Alternative: Use rlm_load + rlm_exec for agent-driven analysis "
                "(no extra dependencies needed)."
            )

        # Apply the openai compatibility patch if needed
        try:
            _patch_openai_compat()
        except Exception:
            pass  # Non-critical, may not be needed

        target = Path(path).resolve()
        if target.is_file():
            context = target.read_text(encoding="utf-8", errors="replace")
        elif target.is_dir():
            # Load all text files
            parts = []
            for f in sorted(target.rglob("*")):
                if f.is_file() and f.suffix in {".py", ".js", ".ts", ".rs", ".go", ".md", ".txt", ".toml", ".yaml", ".yml", ".json"}:
                    try:
                        text = f.read_text(encoding="utf-8", errors="replace")
                        rel = f.relative_to(target)
                        parts.append(f"# === {rel} ===\n{text}")
                    except (OSError, UnicodeDecodeError):
                        continue
            context = "\n\n".join(parts)
        else:
            return f"[ERROR] Path not found: {path}"

        try:
            rlm = RLM(
                backend=self.config.backend,
                backend_kwargs=self.config.backend_kwargs,
                environment=self.config.environment,
                max_iterations=max_iterations,
            )
            result = rlm.completion(
                prompt={"context": context, "question": question},
            )
            return result.response
        except Exception as e:
            return f"[ERROR] RLM analysis failed: {type(e).__name__}: {e}"

    def status(self) -> dict[str, Any]:
        """Return workspace status information."""
        rlm_available = False
        try:
            import rlm  # noqa: F811
            rlm_available = True
        except ImportError:
            pass

        repl_vars = self.repl.list_vars()
        total_loaded = sum(
            info.get("size", 0) for info in repl_vars.values()
            if isinstance(info, dict)
        )

        return {
            "project_root": str(self.config.project_root),
            "backend": self.config.backend,
            "model": self.config.backend_kwargs.get("model_name", "unknown"),
            "base_url": self.config.backend_kwargs.get("base_url", "unknown"),
            "environment": self.config.environment,
            "max_iterations": self.config.max_iterations,
            "repl_timeout": self.config.repl_timeout,
            "variables_loaded": len(repl_vars),
            "total_content_chars": total_loaded,
            "rlm_library_available": rlm_available,
            "autonomous_mode": "available" if rlm_available else "unavailable (install rlms)",
            "agent_driven_mode": "available",
        }


def _patch_openai_compat() -> None:
    """Apply openai 2.x pydantic compatibility patch if needed."""
    try:
        import openai._compat as _oai_compat
    except ImportError:
        return

    _original = _oai_compat.model_dump

    def _patched(model: Any, **kwargs: Any) -> Any:
        if kwargs.get("by_alias") is None:
            kwargs["by_alias"] = False
        return _original(model, **kwargs)

    _oai_compat.model_dump = _patched
    for mod_path in ["openai._base_client", "openai._utils._transform", "openai._utils._json"]:
        try:
            import importlib
            mod = importlib.import_module(mod_path)
            if hasattr(mod, "model_dump"):
                mod.model_dump = _patched
        except ImportError:
            pass
