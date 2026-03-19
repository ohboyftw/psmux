#!/usr/bin/env bash
# Cross-platform Python resolver
# Tries 'py' (Windows launcher) first, then 'python3' (Linux/Mac)
if command -v py >/dev/null 2>&1; then
    exec py "$@"
else
    exec python3 "$@"
fi
