"""
Sandboxed Python REPL for agent-driven exploration.

Provides a persistent namespace where Claude Code can load content as variables
and execute Python code to explore it. Restricted builtins prevent filesystem
writes, subprocess calls, and other dangerous operations.
"""

from __future__ import annotations

import builtins
import io
import threading
from contextlib import redirect_stdout, redirect_stderr
from typing import Any


# Builtins that are explicitly blocked in the sandbox
_BLOCKED_BUILTINS = frozenset({
    "eval", "exec", "compile",
    "__import__",  # we provide a restricted import instead
    "breakpoint", "exit", "quit",
})

# Modules allowed for import inside the sandbox
_ALLOWED_MODULES = frozenset({
    "re", "json", "math", "statistics", "collections", "itertools",
    "functools", "operator", "textwrap", "difflib", "hashlib",
    "datetime", "pathlib", "string", "unicodedata", "pprint",
    "dataclasses", "typing", "enum", "copy", "io", "csv",
    "ast", "tokenize", "keyword", "bisect", "heapq",
    "Counter", "defaultdict", "OrderedDict",
})


def _restricted_import(name: str, *args: Any, **kwargs: Any) -> Any:
    """Import guard that only allows safe stdlib modules."""
    top_level = name.split(".")[0]
    if top_level not in _ALLOWED_MODULES:
        raise ImportError(
            f"Module '{name}' is not allowed in the sandbox. "
            f"Allowed: {', '.join(sorted(_ALLOWED_MODULES))}"
        )
    return builtins.__import__(name, *args, **kwargs)


def _restricted_open(path: str, mode: str = "r", *args: Any, **kwargs: Any) -> Any:
    """Open guard that only allows read mode."""
    if any(c in mode for c in "wxa+"):
        raise PermissionError("Writing files is not allowed in the sandbox. Use read mode only.")
    return open(path, mode, *args, **kwargs)


def _build_safe_builtins() -> dict[str, Any]:
    """Create a restricted builtins dict for the sandbox."""
    safe = {}
    for name in dir(builtins):
        if name.startswith("_") and name != "__name__":
            continue
        if name in _BLOCKED_BUILTINS:
            continue
        safe[name] = getattr(builtins, name)

    # Override with restricted versions
    safe["__import__"] = _restricted_import
    safe["open"] = _restricted_open
    safe["print"] = print  # will be captured by redirect_stdout
    safe["__name__"] = "__repl__"
    safe["__builtins__"] = safe  # self-reference needed for exec

    return safe


class SandboxedREPL:
    """
    A sandboxed Python execution environment with persistent namespace.

    Variables loaded via the workspace persist across exec calls. Output is
    captured and returned. Dangerous operations are blocked.
    """

    def __init__(self, timeout: float = 30.0):
        self.timeout = timeout
        self._namespace: dict[str, Any] = {}
        self._safe_builtins = _build_safe_builtins()
        # Pre-seed namespace with builtins access
        self._namespace["__builtins__"] = self._safe_builtins

    def set_var(self, name: str, value: Any) -> None:
        """Set a variable in the REPL namespace."""
        self._namespace[name] = value

    def get_var(self, name: str) -> Any:
        """Get a variable from the REPL namespace."""
        return self._namespace.get(name)

    def has_var(self, name: str) -> bool:
        """Check if a variable exists in the namespace."""
        return name in self._namespace

    def list_vars(self) -> dict[str, dict[str, Any]]:
        """List all user-set variables with metadata."""
        result = {}
        for name, value in self._namespace.items():
            if name.startswith("__"):
                continue
            info: dict[str, Any] = {"type": type(value).__name__}
            if isinstance(value, str):
                info["size"] = len(value)
                info["lines"] = value.count("\n") + 1
            elif isinstance(value, (list, tuple, dict, set)):
                info["length"] = len(value)
            result[name] = info
        return result

    def clear(self) -> None:
        """Clear all user variables from the namespace."""
        keys_to_remove = [k for k in self._namespace if not k.startswith("__")]
        for k in keys_to_remove:
            del self._namespace[k]

    def execute(self, code: str) -> dict[str, str]:
        """
        Execute Python code in the sandbox.

        Returns dict with 'stdout' and 'error' keys.
        Enforces timeout to prevent runaway execution.
        """
        stdout_buf = io.StringIO()
        stderr_buf = io.StringIO()
        result: dict[str, str] = {"stdout": "", "error": ""}
        exception_holder: list[Exception] = []

        def _run() -> None:
            try:
                with redirect_stdout(stdout_buf), redirect_stderr(stderr_buf):
                    exec(code, self._namespace)  # noqa: S102 — intentionally sandboxed
            except Exception as e:
                exception_holder.append(e)

        thread = threading.Thread(target=_run, daemon=True)
        thread.start()
        thread.join(timeout=self.timeout)

        if thread.is_alive():
            result["error"] = f"Execution timed out after {self.timeout}s"
            return result

        result["stdout"] = stdout_buf.getvalue()

        if exception_holder:
            exc = exception_holder[0]
            result["error"] = f"{type(exc).__name__}: {exc}"

        stderr_output = stderr_buf.getvalue()
        if stderr_output and not result["error"]:
            result["error"] = stderr_output

        return result
