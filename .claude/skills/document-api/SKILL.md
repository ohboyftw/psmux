---
name: document-api
description: Generate or update documentation for psmux — README sections, doc comments, man-page-style help text, and configuration reference. Use when the user says "document", "update docs", "add docs", "write readme", "update readme", "help text", "man page", "document this command", or when any new command or feature needs documentation.
---

# Documenting psmux

## Documentation Locations

1. **README.md** — Primary user-facing docs (installation, usage, keybindings, scripting)
2. **Rust doc comments** (`///`) — API docs for all public items
3. **`--help` output** — CLI help text built into the binary
4. **`psmux.json`** — Configuration schema (self-documenting)
5. **`~/.psmux.conf`** — Configuration file format docs (in README)

## README Structure (follow existing)
The README has these major sections — add new content to the appropriate one:
- Installation (Quick Install, Cargo, Chocolatey, GitHub Releases, Source)
- Usage (basic commands)
- Key Bindings (prefix key table + copy mode table)
- Scripting & Automation (window/pane control, send-keys, pane info, buffers, layout, session)
- Format Variables
- Configuration
- FAQ

## Writing Doc Comments

Every public function, struct, and enum must have `///` doc comments:

```rust
/// Splits the current pane in the given direction.
///
/// # Arguments
/// * `direction` - Horizontal (side-by-side) or Vertical (top/bottom)
/// * `size` - Optional percentage or fixed cell count for the new pane
///
/// # Errors
/// Returns an error if the pane is too small to split further.
pub fn split_pane(direction: SplitDirection, size: Option<PaneSize>) -> Result<PaneId> {
```

## --help Text Convention

Follow this pattern for subcommand help:
```
psmux split-window [-h | -v] [-l size] [-t target-pane]

  Split the current pane into two.

Options:
  -h          Split horizontally (side by side)
  -v          Split vertically (top and bottom) [default]
  -l size     Size of the new pane (lines or percentage)
  -t target   Target pane identifier

Examples:
  psmux split-window -h
  psmux split-window -v -l 30%
```

## Documenting New Commands

When a new command is added:
1. Add to the README in the appropriate subsection under "Scripting & Automation"
2. Add keybinding to the "Key Bindings" table if applicable
3. Add doc comments to the Rust handler function
4. Update `--help` output for the command
5. Add any new format variables to the "Format Variables" table

## Configuration Documentation
When adding a new config option:
1. Document in README under "Configuration" with example
2. Add to `psmux.json` schema
3. Note the default value and valid range
