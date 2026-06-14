# TRAWL — Requirements

> **TRAWL**: TODO Repository Annotation Work List
>
> *trawl* (verb): to fish with a dragging net; to sift through something
> thoroughly. From Middle English *trawlen*, via Middle Dutch *tragelen*
> (to drag). The tool drags a net through your repository and catches
> every annotation — TODOs, FIXMEs, goal trackers, checklists.

## Overview

TRAWL is a TUI tool for discovering and visualizing work items
embedded in a repository. It scans files for two types of annotations:

- **Goals & Milestones**: structured, multi-week objectives tracked via
  `## GOAL TRACKER` sections in markdown files (courses, books, projects)
- **Inline Tasks**: inline markers in any file (`TODO`, `FIXME`, `HACK`,
  etc.)

Both are **scanned and auto-discovered** — the user does not register or
configure individual items. They differ in syntax and granularity, not in
discovery method.

The tool renders an interactive terminal dashboard with a hierarchical,
foldable directory tree for inline tasks and a progress-oriented view for
goals. It is designed to be **repo-agnostic**: it works on knowledge bases,
code repositories, configuration repos, or any git-tracked project.

### Design Principles

- **Scan, don't manage**: TRAWL discovers items from file contents. It does
  not store state in a database — the files *are* the database.
- **Pure markdown + inline markers**: no frontmatter, no custom file format.
  Goal trackers are standard markdown sections; inline tasks are standard
  comments.
- **Resilient parsing**: the parser degrades gracefully. Bare `TODO` is
  always valid. Malformed checkbox trees, broken table separators, unknown
  metadata tokens, or unexpected indentation are handled without crashing —
  skip what you cannot parse, parse what you can.
- **Two types, one tool**: goals and inline tasks are independently
  discovered and independently displayed. They are peers, not a hierarchy.
  Do not assume a parent-child relationship between them.
- **Extensible by configuration**: metadata token prefixes (`@`, `#`, `!`,
  `~`) and table column mappings are configurable, not hard-coded.
- **Binary is the product**: the compiled binary is the end-user interface.
  `--help`, the `?` overlay, and error messages are the user contract and
  must stay accurate and complete; the source repo is contributor
  documentation.

## Two Types of Annotations

TRAWL recognizes two types of work items:

```
┌──────────────────────────────────────────────────────────────────┐
│ GOALS & MILESTONES  "Complete CS146s"            weeks/months    │
│                     Syntax: ## GOAL TRACKER sections in markdown │
│                     Display: progress dashboard, checkbox tree   │
│                     Tracking: % complete (from checkbox ratio)   │
├──────────────────────────────────────────────────────────────────┤
│ INLINE TASKS        "TODO: add cache examples"   minutes/hours   │
│                     Syntax: inline markers (TODO, FIXME, etc.)   │
│                     Display: hierarchical directory tree          │
│                     Tracking: exists/removed, age via git blame  │
└──────────────────────────────────────────────────────────────────┘
```

### Why Two Types?

Goals and inline tasks are fundamentally different data:

| Aspect | Goals & Milestones | Inline Tasks |
|--------|-------------------|--------------|
| Granularity | Days to months | Minutes to hours |
| Source | Authored sections in markdown | Inline markers in any file |
| Volume | A handful at a time | Dozens to hundreds |
| Lifecycle | Tracked through milestones | Resolved by editing the line |
| Structure | Hierarchical (checkbox tree) | Flat (file:line) |
| Progress | Percentage (3/10 tasks done) | Binary (exists or removed) |

### Goals and Inline Tasks Are Independent

Goals and inline tasks are **peers**, not a hierarchy. They are
independently discovered and independently displayed. Many inline tasks
are orphan — minor tasks with no associated goal. Many goals have no
inline tasks in their directory. The tool does **not** assume a structural
parent-child relationship between them.

The only cross-reference is an **optional proximity filter**: when viewing
a goal, the user can optionally ask "show inline tasks in this goal's
directory." This is a convenience filter, not a structural link. Directory
structure does not reliably map to goal scope, and the tool never assumes
it does.

## Goal Tracker

Goals are structured progress trackers embedded in markdown files via a
`## GOAL TRACKER` section. The section contains checkbox lists or tables
representing milestones and tasks.

**Example:**

```markdown
# Complete CS146s-2025

## GOAL TRACKER

- [x] Week 1: Introduction
  - [x] Lecture 1: How an LLM is made
  - [x] Assignment 1: Basic prompting
  - [ ] Reading: Prompt Engineering Guide !low
- [ ] Week 2: Power Prompting
  - [ ] Lecture 3 !high @yiqun
  - [ ] Assignment 2
```

### Milestones and Tasks

The checkbox tree *is* the hierarchy. No special markers needed:

- A checkbox item **with children** (indented sub-items) = **milestone**
- A checkbox item **without children** = **task**
- Deeper nesting creates sub-tasks naturally

`###` and other headings within the GOAL TRACKER section are treated as
visual formatting — ignored by the parser, just like any other
non-checkbox content.

### Key Properties

- **Section-scoped**: only content within `## GOAL TRACKER` is parsed.
  The rest of the file is free-form notes, ignored by the parser.
- **Two interchangeable formats**: checkbox lists and tables, freely
  mixed within the same section.
- **Minimal metadata**: everything is derived — title from H1, progress
  from checkbox ratio, status from progress.
- **Extensible**: metadata tokens (`@owner`, `#tag`, etc.) can be embedded
  in task text; table columns are auto-mapped by header name.

### Badge (Location)

Each goal displays a **location badge** derived from its file path:
the relative path of the **super directory** of the file's owning
directory.

| File location | Owning dir | Super dir | Badge |
|---------------|-----------|-----------|-------|
| `ml/llm/stanford-cs146s-2025/README.md` | `ml/llm/stanford-cs146s-2025/` | `ml/llm/` | `ml/llm/` |
| `misc/books/systems-performance/ch13.md` | `misc/books/systems-performance/` | `misc/books/` | `misc/books/` |
| `README.md` (repo root) | `.` (root) | — | `(root)` |

### Derived Fields

| Field | Source | Example |
|-------|--------|---------|
| Title | First H1 in file (fallback: filename) | "Complete CS146s-2025" |
| Progress | `count(done leaf tasks) / count(all leaf tasks)` | 40% |
| Status | Auto from progress | `completed` / `active` / `planned` |

> Full syntax specification: see [syntax.md](syntax.md)

## Inline Tasks

Inline tasks are markers discovered by scanning file contents. They
represent small, atomic tasks embedded directly in code or notes.

**Format:**

```
TODO(scope): description @owner #tag !priority ~due
```

All components except the keyword and description are optional. Bare
`TODO: text` is always valid, making existing markers immediately
parseable.

**Supported keywords:** `TODO`, `FIXME`, `HACK`, `XXX`, `BUG`, `NOTE`
(configurable).

**Example across contexts:**

```c
/* TODO(auth): handle null user @yiqun #security !high ~2025-12-01 */
```

```python
# TODO(perf): optimize scan loop !med
```

```markdown
TODO: add examples for cache types #arch
```

> Full syntax specification: see [syntax.md](syntax.md)

## Feature Catalog

### Scanner

| Feature | Description |
|---------|-------------|
| Recursive walk | Walk entire repo tree from root |
| `.gitignore` awareness | Skip ignored files (via `ignore` crate) |
| Binary detection | Skip non-text files (null byte heuristic) |
| File type filtering | Configurable include/exclude globs |
| Max file size | Skip files above threshold (default: 1 MB) |
| Parallel scanning | Multi-threaded file reading (via `rayon`) |
| Both types in one pass | Goals and inline tasks discovered in the same scan |
| Keyword matching | Finds keyword lines via the `regex` crate; `grep-searcher`/`grep-regex` deferred as a later optimization for very large monorepos |

### TUI — Goals & Milestones View

Dashboard showing all discovered goals with progress bars, location
badges, and derived status. Expand a goal inline to see its full
checkbox tree (milestones, tasks, sub-tasks).

```
┌──────────────────────────────────────────────────────────────┐
│ GOALS & MILESTONES                              [Tab: Inline Tasks] │
├──────────────────────────────────────────────────────────────┤
│  ▼ ★ CS146s-2025                  ml/llm/    33%             │
│    [x] Week 1: Introduction                  2/3             │
│      [x] Lecture 1: How an LLM is made                       │
│      [x] Assignment 1: Basic prompting                        │
│      [ ] Reading: Prompt Engineering Guide !low              │
│    [ ] Week 2: Power Prompting               0/2             │
│      [ ] Lecture 3 !high @yiqun                               │
│      [ ] Assignment 2                                         │
│    [ ] Buy textbook                                           │
│  ▸ ★ Understanding Linux VM Manager  misc/books/  90%        │
│  ▸ ★ Sprint 15: Auth Refactor         (root)       55%       │
├──────────────────────────────────────────────────────────────┤
│ Enter: expand  Space: toggle  e: edit  Tab: Inline Tasks     │
└──────────────────────────────────────────────────────────────┘
```

**Behaviors:**

- `Enter` expands/collapses a goal to show its checkbox tree inline
- `Space` toggles a checkbox (writes `[x]`/`[ ]` back to the file)
- `j/k` navigates the tree (goals, milestones, tasks)
- Milestones show per-group ratio (e.g., `2/3`)
- Color coding: done items dimmed, high-priority items highlighted
- Completed goals render dimmed and struck-through; press `C` to collapse
  every completed goal at once

### TUI — Inline Tasks View

Hierarchical, foldable directory tree showing every inline task
organized by file path. Each item displays line number, keyword,
description, and optional priority badge.

```
┌──────────────────────────────────────────────────────────────┐
│ INLINE TASKS  •  127 items  •  3 stale          [Tab: Goals] │
├──────────────────────────────────────────────────────────────┤
│ STATS  high:5  med:23  low:99  untagged:42  stale:3          │
├──────────────────────────────────────────────────────────────┤
│ ▼ ml/llm/stanford-cs146s-2025/  [7]                          │
│   ▸ 01-introduction-and-how-an-llm-is-made.md  [1]           │
│   ▸ w1-readings.md  [1]                                       │
│ ▼ impl/  [48]                                                │
│   ▼ kernel/  [41]                                            │
│     ▸ memory-management/  [19]                               │
│ ▼ tool/  [33]                                                │
├──────────────────────────────────────────────────────────────┤
│ Enter: expand  f: filter  s: sort  Tab: Goals  q: quit       │
└──────────────────────────────────────────────────────────────┘
```

**Tree behaviors:**

- Fold/unfold with `Enter` or `l/h` (vim-style)
- Item counts `[N]` next to each dir/file node
- Inline preview of each task with line number and keyword
- Color coding: priority (red/yellow/gray), keyword (cyan/red/etc.)
- Optional git blame line (toggle with `g`)
- Auto-expand directories containing high-priority tasks

### Inline Expansion

Pressing `Enter` on a specific inline task **expands it in place** within
the tree, showing 2 context lines (configurable via `context_lines`) from
the source file and optional git blame below it. Press `Enter` again to
collapse. No separate panel or view switch — the user keeps their position
in the tree.

### Filtering

| Filter | Description |
|--------|-------------|
| Keyword | Only `FIXME` items, etc. |
| Tag | Only `#security` tagged items |
| Priority | Only `!high` items |
| Owner | Only `@yiqun` items |
| Full-text search | Substring search in descriptions |
| Stale only | Items older than threshold (default: 365 days) |
| Path glob | `impl/kernel/**/*.c` |

### Sorting

| Sort | Description |
|------|-------------|
| Priority | high → med → low → untagged |
| Age | Oldest first (highlights stale debt) |
| File path | Alphabetical (default) |
| Keyword | Group all FIXMEs, then TODOs, etc. |

### Stats Dashboard

Toggle from either view to see aggregate statistics:

- Counts by priority, keyword, and tag (bar charts)
- Top directories by task count
- Stale task count (older than threshold)

### Git Integration

| Feature | Description |
|---------|-------------|
| Blame enrichment | Author, date, commit hash for each item |
| Age calculation | Days since item was added |
| Stale detection | Flag items older than configurable threshold |

### Help System

Press `?` at any time to open a context-sensitive help overlay showing
all keybindings for the current view. Different views have different
keybindings — the overlay adapts.

```
┌──────────────────────────────────────────────────┐
│  TRAWL — Keybindings             [?] close help  │
├──────────────────────────────────────────────────┤
│  Navigation                                      │
│    j / k        move down / up                   │
│    l / h        expand / collapse                │
│    Enter        toggle expand (inline context)   │
│    Space        toggle checkbox (in goals)       │
│    Tab          switch Goals ↔ Inline Tasks      │
│    g            toggle git blame                 │
│                                                  │
│  Filtering & Sorting                             │
│    f            open filter menu                 │
│    s            cycle sort mode                  │
│    /            search by text                   │
│                                                  │
│  Other                                           │
│    C            collapse completed goals         │
│    S            stats dashboard                  │
│    e            edit file at cursor              │
│    q            quit                             │
└──────────────────────────────────────────────────┘
```

### Editor Integration

The `e` key suspends the TUI and launches `$EDITOR` (or `$VISUAL`) at
the cursor's file and line number. The TUI resumes when the editor
exits. Falls back to `vi` (Unix) or `notepad` (Windows) if neither
variable is set.

## Configuration

Configuration is layered — later sources override earlier:

```
~/.config/trawl/config.toml        ← user global defaults
<repo>/.trawl.toml                 ← project-level overrides
```

### Project-Level Config (`.trawl.toml`)

A `.trawl.toml` file in the repository root provides per-project
overrides. This is essential for repos like trawl itself, where docs and
test fixtures contain marker patterns that must be excluded from
scanning:

```toml
# .trawl.toml — exclude trawl's own docs and test fixtures
[scan]
exclude = ["docs/", "tests/fixtures/"]
```

### Full Config Reference

```toml
[scan]
keywords = ["TODO", "FIXME", "HACK", "XXX", "BUG", "NOTE"]
keyword_case_sensitive = false
goal_section_names = ["GOAL TRACKER"]
include = []  # e.g. ["*.md", "*.rs", "*.py"] — restrict to specific file types
exclude = ["target/", "node_modules/", ".git/"]  # built-in defaults; project config merges with these (union)
max_file_size = "1MB"
scan_hidden = false

[tokens]
owner = "@"
tag = "#"
priority = "!"
due = "~"

[headers]
task = ["task", "item", "name", "todo", "work"]
state = ["state", "status", "done", "progress", "check"]
owner = ["owner", "assignee", "who"]
priority = ["priority", "pri"]
tag = ["tag", "category", "label"]
due = ["due", "deadline", "target"]

[display]
default_sort = "path"
show_git_blame = false
context_lines = 2
auto_expand_priority = "high"
stale_threshold_days = 365
```

### Scan Filtering Semantics

File filtering uses a layered pipeline — each stage narrows the set of
files passed to the scanner:

| Stage | Source | Behavior |
|-------|--------|----------|
| 1. `.gitignore` | Implicit | Git-ignored files are never scanned |
| 2. `exclude` | Config | Files/dirs matching any glob are skipped (blacklist) |
| 3. `include` | Config | When non-empty, only matching files are scanned (whitelist). When empty (default), no extension restriction applies |
| 4. `scan_hidden` | Config | When `false`, dotfiles and dot-directories are skipped |
| 5. `max_file_size` | Config | Files exceeding the threshold are skipped |
| 6. Binary detection | Heuristic | Files containing null bytes are skipped |

`exclude` is always applied (blacklist). `include` is an optional
whitelist — when omitted or empty, the tool scans all non-ignored,
non-excluded text files, making it repo-agnostic out of the box. The
`include` value shown in the config reference above is illustrative, not
the default.

`exclude` and `include` set in project config **merge** with the built-in
defaults — they extend the default sets (union, de-duplicated) rather than
replacing them. A project that sets `exclude = ["docs/"]` still skips
`target/` and the other built-in defaults; it does not have to re-list them.

All fields are optional with sensible defaults. The `[tokens]` and
`[headers]` sections are **extensible** — users can add custom metadata
types (e.g., `effort = "%"` for effort estimation).

## Phased Implementation Plan

```
Phase 1 — MVP                         Phase 2 — Interaction
─────────────────────                 ─────────────────────
scanner (walker + reader)             filtering (tag/pri/owner/text)
inline task parser                    sorting (priority/age/path/keyword)
goal tracker parser                   inline expansion (context lines)
basic TUI tree (fold/unfold)          color coding + priority badges
TUI goals view (checkbox tree)        stats bar (counts)
keyboard nav (j/k/l/h/Enter/q)        checkbox toggle (Space)
Tab to switch views                   help overlay (?)
                                      editor integration (e)

Phase 3 — Git + Stats
─────────────────────
git blame enrichment
stale detection
stats dashboard view
```

## Key Crates

| Crate | Purpose |
|-------|---------|
| `ratatui` + `crossterm` | TUI rendering + terminal backend |
| `clap` | CLI argument parsing |
| `ignore` | `.gitignore`-aware directory walking |
| `regex` | Keyword line matching (fast enough at repo scale) |
| `grep-searcher` + `grep-regex` | Ripgrep-class matching — planned optimization for very large monorepos |
| `serde` + `toml` | Config file deserialization |
| `git2` | libgit2 bindings for blame |
| `chrono` | Date handling |
| `rayon` | Parallel file scanning |
