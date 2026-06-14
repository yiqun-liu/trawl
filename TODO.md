# TRAWL — Implementation Tracker

This document tracks planned features and implementation progress.
It uses the project's own goal tracker syntax for dogfooding.

For feature requirements and design principles, see `docs/requirements.md`.
For syntax specification, see `docs/syntax.md`.

| Feature | Phase | Description |
|---------|-------|-------------|
| Project Scaffold | 1 | Cargo.toml, module layout, CLI parsing, config loading |
| Scanner | 1 | `.gitignore`-aware walker, binary detection, parallel scanning |
| Inline Task Parser | 1 | Keyword extraction, metadata token parsing, scope parsing |
| Goal Tracker Parser | 1 | Section detection, checkbox tree, table format, progress |
| TUI Framework | 1 | ratatui setup, event loop, Tab view switching |
| TUI Goals View | 1 | Checkbox tree rendering, progress bars, badges |
| TUI Inline Tasks View | 1 | Directory tree, fold/unfold, item counts |
| Keyboard Navigation | 1 | j/k/l/h/Enter/Space/q/Tab keybindings |
| Filtering | 2 | Keyword/tag/priority/owner/text/path/stale filters |
| Sorting | 2 | Priority/age/path/keyword sort modes |
| Inline Expansion | 2 | Context lines display within tree on Enter |
| Color Coding | 2 | Priority and keyword color schemes, badges |
| Checkbox Toggle | 2 | Space writes `[x]`/`[ ]` back to source file |
| Stats Bar | 2 | Aggregate counts in inline tasks view header |
| Help Overlay | 2 | Context-sensitive keybinding display (`?`) |
| Editor Integration | 2 | Suspend TUI and launch `$EDITOR`/`$VISUAL` at file:line |
| Git Blame Enrichment | 3 | Author/date/commit hash per inline task |
| Stale Detection | 3 | Age-based flagging with configurable threshold |
| Stats Dashboard | 3 | Aggregate statistics with bar charts, top directories |

## GOAL TRACKER

### Phase 1 — MVP

- [x] Project Scaffold
  - [x] Create `Cargo.toml` with dependencies (ratatui, crossterm, clap, ignore, regex, serde, toml, rayon, git2, chrono, anyhow, log, env_logger)
  - [x] Define module layout (`src/scanner/`, `src/parser/`, `src/tui/`, `src/config.rs`, `src/main.rs`)
  - [x] Implement CLI argument parsing with clap (`--path`, `--verbose`, etc.)
  - [x] Implement layered config loading (`~/.config/trawl/config.toml` + `<repo>/.trawl.toml`)
  - [x] Define core data types (`Goal`, `TaskNode`, `InlineTask`, `Metadata`)
  - [x] Add `rustfmt.toml` and verify `cargo fmt` / `cargo clippy` pass
- [x] Scanner
  - [x] Implement recursive directory walk using `ignore` crate
  - [x] Implement binary detection (null byte heuristic)
  - [x] Implement file type filtering via layered pipeline
  - [x] Implement parallel file reading with `rayon`
  - [x] Wire scanner output to both parsers in a single pass
  - [x] Implement keyword line matching via `regex` crate (`grep-searcher` deferred to optimization)
- [x] Inline Task Parser
  - [x] Implement keyword matching (configurable keyword list)
  - [x] Implement scope extraction `(name)`
  - [x] Implement metadata token extraction (`@owner`, `#tag`, `!priority`, `~due`)
  - [x] Implement backward-compatible parsing (fully structured to bare keyword)
  - [x] Implement markdown heading stripping (`## TODO:` to `TODO:`)
  - [x] Apply keyword default priorities (`FIXME` to high, `HACK` to med, etc.)
  - [x] Add test fixtures for all five parsing forms
  - [x] Add test fixtures for all comment contexts (C, Python, shell, markdown)
- [x] Goal Tracker Parser
  - [x] Implement section detection (heading match, section boundary by heading level)
  - [x] Implement checkbox tree parsing (indentation-based hierarchy)
  - [x] Implement milestone vs task distinction (has children = milestone)
  - [x] Implement table format parsing (column detection by header keywords)
  - [x] Implement table done heuristic (`NOT (empty OR contains "TODO")`)
  - [x] Implement mixed format (checkbox lists + tables in same section)
  - [x] Implement metadata token extraction from task descriptions and table cells
  - [x] Implement progress calculation (leaf task ratio)
  - [x] Implement derived fields (title from H1, status from progress)
  - [x] Implement location badge derivation (super directory path)
  - [x] Add test fixtures for checkbox trees, tables, mixed format, edge cases
- [x] TUI Framework
  - [x] Initialize ratatui terminal with crossterm backend
  - [x] Implement event loop (keyboard input via crossterm events)
  - [x] Implement Tab-based view switching (Goals and Inline Tasks)
  - [x] Implement app state struct (current view, filter state, sort mode)
  - [x] Implement clean terminal restoration on exit / panic
- [ ] TUI Goals View
  - [x] Render goal list (title, location badge, progress %)
  - [x] Render progress bars for each goal
  - [x] Implement inline expand/collapse of checkbox tree
  - [x] Render milestone per-group ratios (e.g., `2/3`)
  - [ ] Render task metadata (priority badge, owner, tags)
- [x] TUI Inline Tasks View
  - [x] Build directory tree structure from scan results
  - [x] Render foldable tree nodes (directories, files, tasks)
  - [x] Display item counts `[N]` next to each dir/file node
  - [x] Render task details (line number, keyword, description)
  - [x] Implement auto-expand for directories containing high-priority tasks
- [x] Keyboard Navigation
  - [x] `j`/`k` — move down/up
  - [x] `l`/`h` — expand/collapse (vim-style)
  - [x] `Enter` — toggle expand (inline context for tasks)
  - [x] `Space` — toggle checkbox (in goals view)
  - [x] `Tab` — switch Goals and Inline Tasks
  - [x] `q` — quit

### Phase 2 — Interaction

- [ ] Filtering
  - [x] Implement keyword filter (only `FIXME`, etc.)
  - [x] Implement tag filter (only `#security`, etc.)
  - [x] Implement priority filter (only `!high`, etc.)
  - [x] Implement owner filter (only `@yiqun`, etc.)
  - [x] Implement full-text search (substring in descriptions)
  - [ ] Implement stale-only filter (depends on Phase 3 stale detection)
  - [ ] Implement path glob filter (`impl/kernel/**/*.c`) — substring match for now
  - [x] Add filter UI (popup or prompt)
- [ ] Sorting
  - [ ] Implement priority sort (high to med to low to untagged)
  - [ ] Implement age sort (oldest first — depends on Phase 3 git blame)
  - [ ] Implement file path sort (alphabetical, default)
  - [ ] Implement keyword sort (group all FIXMEs, then TODOs, etc.)
  - [ ] Add sort mode cycling (`s` key)
- [ ] Inline Expansion
  - [ ] Read context lines from source file around task line number
  - [ ] Render expanded view inline within the tree
  - [ ] Preserve cursor position during expand/collapse
- [x] Color Coding
  - [x] Define priority color scheme (red/yellow/gray)
  - [x] Define keyword color scheme (cyan for TODO, red for FIXME, etc.)
  - [x] Dim completed goal tasks
  - [x] Highlight high-priority items
- [x] Checkbox Toggle
  - [x] Implement targeted file write (modify only the checkbox character)
  - [x] Update in-memory tree state after write
  - [x] Recompute progress for affected goal and parent milestones
  - [x] Handle file write errors gracefully
- [x] Stats Bar
  - [x] Count tasks by priority (high/med/low/untagged)
  - [ ] Count stale tasks (depends on Phase 3)
  - [x] Render stats line in TUI header
- [x] Help Overlay
  - [x] Define keybinding sets per view (goals, inline, stats)
  - [x] Render help overlay as a centered popup
  - [x] Close on `?` or `Esc`
- [x] Editor Integration
  - [x] Suspend TUI and save terminal state on `e` keybinding
  - [x] Spawn `$EDITOR`/`$VISUAL` at cursor's file and line number
  - [x] Fall back to `vi` (Unix) or `notepad` (Windows) if neither variable is set
  - [x] Restore TUI state on editor exit

### Phase 3 — Git + Stats

- [ ] Git Blame Enrichment
  - [x] Implement blame lookup using `git2` crate
  - [x] Store blame data alongside inline task results
  - [ ] Display blame line in inline expansion view
  - [ ] Add `g` keybinding to toggle blame display
- [x] Stale Detection
  - [x] Calculate age from blame commit date
  - [x] Apply configurable stale threshold (`stale_threshold_days`)
  - [x] Mark stale items in scan results
  - [x] Surface stale count in stats bar
- [ ] Stats Dashboard
  - [ ] Count tasks by priority, keyword, and tag
  - [ ] Calculate top directories by task count
  - [ ] Render bar charts in TUI
  - [ ] Add `S` keybinding to toggle stats view

## Details

### Project Scaffold

**Source:** `docs/requirements.md` → Key Crates; `docs/requirements.md` → Configuration

Initialize the Rust project with module structure, CLI argument parsing, and
layered config loading.

**Dependencies:** none

**Impact:** Foundation for all subsequent work

---

### Scanner

**Source:** `docs/requirements.md` → Feature Catalog → Scanner; `docs/requirements.md` → Scan Filtering Semantics

Walk the repository tree, read file contents, and pass data to the parsers.
Must respect `.gitignore`, skip binary files, and support configurable
include/exclude filtering via the layered pipeline:
`.gitignore` → `exclude` → `include` → `scan_hidden` → `max_file_size` → binary.

**Dependencies:** Project Scaffold

**Impact:** All parser and TUI work depends on the scanner

---

### Inline Task Parser

**Source:** `docs/syntax.md` → Inline Task Syntax; `docs/syntax.md` → Shared Metadata Tokens

Extract inline task markers (`TODO`, `FIXME`, `HACK`, etc.) from file
contents. Parse keyword, optional scope, description, and metadata tokens.
Must degrade gracefully — bare `TODO` is always valid.

**Dependencies:** Project Scaffold (data types)

**Impact:** Powers the inline tasks TUI view

---

### Goal Tracker Parser

**Source:** `docs/syntax.md` → Goal Tracker Syntax; `docs/requirements.md` → Goal Tracker

Parse `## GOAL TRACKER` markdown sections into hierarchical checkbox trees
and task tables. Compute progress, status, and derived fields.

**Dependencies:** Project Scaffold (data types)

**Impact:** Powers the goals TUI view; most complex parser component

---

### TUI Framework

**Source:** `docs/requirements.md` → Feature Catalog

Set up the ratatui + crossterm terminal application with an event loop and
view-switching infrastructure.

**Dependencies:** Project Scaffold

**Impact:** Required before any view implementation

---

### TUI Goals View

**Source:** `docs/requirements.md` → Feature Catalog → TUI — Goals & Milestones View

Dashboard showing all discovered goals with progress bars, location badges,
and derived status. Expand a goal inline to see its full checkbox tree.

**Dependencies:** Goal Tracker Parser, TUI Framework

**Impact:** Primary user-facing view for goals

---

### TUI Inline Tasks View

**Source:** `docs/requirements.md` → Feature Catalog → TUI — Inline Tasks View

Hierarchical, foldable directory tree showing every inline task organized by
file path. Each item displays line number, keyword, description, and optional
priority badge.

**Dependencies:** Inline Task Parser, TUI Framework

**Impact:** Primary user-facing view for inline tasks

---

### Keyboard Navigation

**Source:** `docs/requirements.md` → Feature Catalog → Help System

Implement the core keybindings shared across both views.

**Dependencies:** TUI Framework, TUI Goals View, TUI Inline Tasks View

**Impact:** Makes the TUI interactive

---

### Filtering

**Source:** `docs/requirements.md` → Feature Catalog → Filtering

Filter inline tasks by keyword, tag, priority, owner, full-text search, path
glob, and staleness.

**Dependencies:** TUI Inline Tasks View

**Impact:** Essential for managing repos with dozens of inline tasks

---

### Sorting

**Source:** `docs/requirements.md` → Feature Catalog → Sorting

Sort inline tasks by priority, age, file path, or keyword.

**Dependencies:** TUI Inline Tasks View

**Impact:** Better organization of large task lists

---

### Inline Expansion

**Source:** `docs/requirements.md` → Feature Catalog → Inline Expansion

Pressing `Enter` on a specific inline task expands it in place within the
tree, showing 2-3 context lines from the source file.

**Dependencies:** TUI Inline Tasks View

**Impact:** Lets users see task context without leaving the TUI

---

### Color Coding

**Source:** `docs/requirements.md` → Feature Catalog → TUI — Goals & Milestones View; TUI — Inline Tasks View

Apply color schemes based on priority and keyword. Done items dimmed,
high-priority items highlighted.

**Dependencies:** TUI Goals View, TUI Inline Tasks View

**Impact:** Visual triage at a glance

---

### Checkbox Toggle

**Source:** `docs/requirements.md` → Feature Catalog → TUI — Goals & Milestones View

`Space` toggles a checkbox by writing `[x]`/`[ ]` back to the source file.

**Dependencies:** TUI Goals View, Goal Tracker Parser

**Impact:** Makes trawl an interactive tool, not just a viewer

---

### Stats Bar

**Source:** `docs/requirements.md` → Feature Catalog → TUI — Inline Tasks View

Display aggregate counts in the inline tasks view header.

**Dependencies:** TUI Inline Tasks View

**Impact:** Quick overview of task distribution

---

### Help Overlay

**Source:** `docs/requirements.md` → Feature Catalog → Help System

Press `?` at any time to open a context-sensitive help overlay showing all
keybindings for the current view.

**Dependencies:** TUI Framework

**Impact:** Discoverability of keybindings

---

### Editor Integration

**Source:** `docs/requirements.md` → Feature Catalog → Editor Integration

The `e` key suspends the TUI and launches `$EDITOR`/`$VISUAL` at the
cursor's file and line number. The TUI resumes when the editor exits. Falls
back to `vi` (Unix) or `notepad` (Windows) if neither variable is set.

**Dependencies:** TUI Framework

**Impact:** Edit a task in context without leaving trawl

---

### Git Blame Enrichment

**Source:** `docs/requirements.md` → Feature Catalog → Git Integration

Enrich each inline task with git blame data: author, date, and commit hash.

**Dependencies:** Inline Task Parser, Scanner

**Impact:** Enables age calculation, stale detection, and blame display

---

### Stale Detection

**Source:** `docs/requirements.md` → Feature Catalog → Git Integration

Flag inline tasks older than a configurable threshold (default: 365 days).

**Dependencies:** Git Blame Enrichment

**Impact:** Highlights technical debt that may no longer be relevant

---

### Stats Dashboard

**Source:** `docs/requirements.md` → Feature Catalog → Stats Dashboard

Toggle from either view to see aggregate statistics with bar charts and top
directories by task count.

**Dependencies:** TUI Framework, Inline Task Parser

**Impact:** High-level overview of repository task health
