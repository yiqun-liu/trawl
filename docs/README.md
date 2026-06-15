# Documentation

This directory contains project documentation.

## Reference

- `requirements.md` — feature catalog, design principles, configuration
  reference, and phased implementation plan. The authoritative source for
  what trawl does and how it is designed.
- `syntax.md` — formal syntax specification for goal trackers and inline
  tasks. Defines exactly what patterns trawl scans for and how it parses
  them, including grammar rules, metadata tokens, and parsing edge cases.
- `goal-tracker-compatibility.md` — comprehensive reference for writing,
  checking, and normalizing trawl-compatible goal trackers. Covers the
  full scanning pipeline (detection → syntax → metadata → interpretation
  → configuration → verification), default config values, and
  diagnosis. Designed to be fetchable from GitHub by external tools and
  skills.

## Development

- `guidelines.md` — development guidelines and commit message conventions
- `TODO.md` (repository root) — implementation tracker using goal tracker
  syntax, with phase breakdown and per-item details
- `design/architecture.md` — implementation-level design: module layout,
  data model, scanner pipeline, parser strategies, and key tradeoffs

## Agent Guidance

- `AGENTS.md` (repository root) — repository-specific instructions for
  coding agents
- `.AGENTS.md` (repository root, gitignored) — per-developer local
  overrides; read if present before using nearby reference trees
