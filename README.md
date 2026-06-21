# TRAWL

> **TRAWL**: TODO Repository Annotation Work List
>
> *trawl* (verb): to fish with a dragging net; to sift through something
> thoroughly. From Middle English *trawlen*, via Middle Dutch *tragelen*
> (to drag). The tool drags a net through your repository and catches
> every annotation — TODOs, FIXMEs, goal trackers, checklists.

TRAWL is a TUI tool for discovering and visualizing work items
embedded in a repository. It scans files for two types of annotations:

- **Goals & Milestones**: structured objectives tracked via `## GOAL TRACKER`
  sections in markdown files (courses, books, projects)
- **Inline Tasks**: inline markers in any file (`TODO`, `FIXME`, `HACK`,
  `XXX`, `BUG`)

Both are **scanned and auto-discovered** — the user does not register or
configure individual items. The files *are* the database.

This project is licensed under MIT OR Apache-2.0. See `LICENSE-MIT` and
`LICENSE-APACHE`.

## What It Provides

- **Scanner**: `.gitignore`-aware recursive walker with ripgrep-class
  pattern matching and parallel file reading
- **Goal Tracker Parser**: parses `## GOAL TRACKER` markdown sections into
  hierarchical checkbox trees, task tables, structural subsections, and
  cross-document references
- **Inline Task Parser**: extracts `TODO`/`FIXME`/`HACK`/etc. markers from
  any text file, with metadata token support (`@owner`, `#tag`,
  `!priority`, `~due`)
- **TUI Dashboard**: interactive terminal interface with two views — a
  progress-oriented goals view and a foldable directory tree for inline
  tasks
- **Git Integration**: blame enrichment and stale detection for inline
  tasks

## Installation

### Build from source

Prerequisites: Rust toolchain (stable).

```bash
cargo build --release
```

Copy the binary to a directory in your PATH:

```bash
cp ./target/release/trawl ~/.local/bin/
```

The release binary dynamically links libssl, libcrypto, libz, and
libbrotli — all standard on typical Linux desktop distros. If those
libraries are present, copying the binary into your PATH is sufficient.

### cargo install

Not yet available. `cargo install trawl` will work once trawl is
published to crates.io.

## Quick Start

Run trawl in a repository:

```bash
trawl
```

Point trawl at a specific repository:

```bash
trawl --path /path/to/repo
```

## Configuration

Configuration is layered — later sources override earlier:

```
~/.config/trawl/config.toml        ← user global defaults
<repo>/.trawl.toml                 ← project-level overrides
```

A `.trawl.toml` in the repository root provides per-project overrides.
Example:

```toml
# .trawl.toml
[scan]
exclude = ["docs/", "tests/fixtures/"]
```

For the full configuration reference, see `docs/requirements.md`.

## Documentation

- `docs/requirements.md` — feature catalog, design principles, configuration
  reference, and phased implementation plan
- `docs/syntax.md` — formal syntax specification for goal trackers and inline
  tasks
- `docs/goal-tracker-compatibility.md` — comprehensive reference for writing,
  checking, and normalizing trawl-compatible goal trackers (fetchable by
  external tools and skills)
- `docs/guidelines.md` — development guidelines and commit message conventions
- `TODO.md` — implementation progress tracker (goal tracker format)
- `docs/README.md` — documentation index

## Repository Layout

- `docs/`: design documentation (requirements, syntax spec, guidelines)
- `src/`: source code — *planned*
  - `src/scanner/`: directory walker and file reader
  - `src/parser/`: inline task and goal tracker parsers
  - `src/tui/`: terminal user interface
  - `src/config.rs`: layered config loading
- `tests/`: integration and unit tests — *planned*

## Agent Notes

Repository-specific agent guidance lives in `AGENTS.md`.
Developers may keep untracked local agent instructions in `.AGENTS.md`.
