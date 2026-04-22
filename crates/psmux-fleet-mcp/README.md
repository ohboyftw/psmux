# psmux-fleet-mcp

Stdio JSON-RPC server exposing psmux-fleet aggregator tools.

Part of [FleetView](../../docs/superpowers/specs/2026-04-22-fleetview-design.md).
Launches on Claude Code's MCP server config; one JSON-RPC message per line.

Methods: `fleet.list`, `fleet.status`. More land in subsequent commits.
