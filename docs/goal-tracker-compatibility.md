# Goal Tracker Compatibility

**trawl** is a TUI tool that discovers work items embedded in your
repository. It scans markdown files for *goal tracker* sections —
structured progress trackers that represent multi-week objectives like
courses, book chapters, or project milestones — and renders them as an
interactive dashboard with progress percentages, hierarchical folding,
and metadata filtering.

Goal trackers live entirely in your markdown files: no database, no
frontmatter, no special file format. You write a `## GOAL TRACKER`
section, trawl finds it, and the files *are* the database.

This document explains how to make goal trackers work with trawl —
whether you are **writing** new ones, **checking** existing ones for
compatibility, or **normalizing** non-compatible trackers into trawl's
format. It follows trawl's scanning pipeline: detection → syntax →
metadata → interpretation → configuration → verification. Each stage
answers a distinct question, from "can trawl even find my content?" down
to "how do I adapt trawl to my vocabulary?".

**Scope**: this document covers goal trackers only. Inline markers
(`TODO`, `FIXME`, `HACK`, etc. in any source file) are a separate
annotation type and out of scope here — see `docs/syntax.md` for their
syntax.

**Configuration**: trawl is configurable via `.trawl.toml`, a per-project
TOML file in the repository root. Throughout this document, values
labelled "default" can be overridden in `.trawl.toml`. The config
section at the end lists all defaults and explains when overrides are
needed.

---

## Detection

How trawl discovers goal tracker content in your repository.

### File scanning

Trawl walks the repository directory tree and decides which files to
scan. A goal that does not appear is often a file-scanning issue, not a
syntax issue.

Files are **skipped** when any of these conditions hold:

| Condition | Default | Effect |
|-----------|---------|--------|
| Not tracked by git | `only_tracked = true` | Untracked files are invisible to trawl |
| Inside a dot-directory or dotfile | `scan_hidden = false` | `.agents/`, `.opencode/` etc. are skipped |
| Matches an exclude glob | `exclude = ["target/", "node_modules/", ".git/"]` | Excluded directories are skipped (project config **merges** with built-in defaults) |
| Exceeds size limit | `max_file_size = "1MB"` | Large files are skipped |
| Contains null bytes | Heuristic | Binary files are skipped |
| Not in the include whitelist (when set) | `include = []` (empty = all types) | Only whitelisted extensions are scanned |
| Listed in `.gitignore` | Implicit | Git-ignored files are never scanned |

**Diagnosis**: if a goal does not appear, check whether the file passes
every stage above. `git ls-files <path>` confirms tracking; `ls -la`
confirms visibility of dot-paths.

### Section detection

Within a scanned file, trawl looks for a markdown section whose heading
matches a configured section name.

| Rule | Detail |
|------|--------|
| Section names | `GOAL TRACKER` and `TODO` by default (configurable via `goal_section_names`) |
| Matching | **Case-insensitive, exact match** on the heading text after stripping `#` markers. Not substring — `## My TODO list` does not match. |
| Heading level | Any level (`#` through `######`) |
| First-match-only | Only the first matching heading in a file is parsed. A second `## GOAL TRACKER` is ignored. |
| Section boundary | Content extends until the next heading of **same or higher level**, or end of file. |

> **Section name vs. inline keyword overlap**: `TODO` is both a default
> goal-section name and a default inline keyword. By design, a heading
> carrying a keyword is also captured as an inline task (so `## TODO:
> review this` is both a section and a TODO). A bare `## TODO` section
> heading therefore also shows up in inline-task results. Prefer
> `## GOAL TRACKER` for a section that is purely a tracker, reserving
> `## TODO` for headings that double as reminders.

```markdown
# CS146s: The Modern Software Developer

Some intro text.                    ← ignored (outside section)

## GOAL TRACKER                     ← section starts

- [x] Week 1: Introduction          ← parsed (milestone)
  - [x] Lecture 1                   ← parsed (task)

## References                       ← section ends (same level)
- [Stanford CS146s](...)            ← ignored
```

Everything outside the detected section is free-form notes — ignored by
trawl.

### File-level derived fields

Two fields are derived from the file itself, before any section content
is parsed:

| Field | Derivation | Example |
|-------|-----------|---------|
| **Title** | First `# H1` heading in the file. Fallback: filename without extension. | `"CS146s: The Modern Software Developer"` |
| **Badge** | Super-directory of the file's owning directory; `(root)` when at or one level under the root. | `ml/llm/` |

---

## Syntax

What format trawl parses inside a detected section.

Within the tracker section, trawl reads content along **two independent
dimensions** — **format** (how leaf items are written: checkboxes or
tables) and **structure** (how items are grouped into a tree). Paragraphs,
images, code blocks, and blank lines are ignored — letting you mix context
notes and visual formatting freely.

### Format and structure

**Format** — how leaf items are written:

| Format | Best for | Trade-offs |
|--------|----------|------------|
| **Checkbox list** | Simple tracking, hierarchical goals (courses, book chapters, multi-phase projects) | Naturally expresses milestones → tasks via indentation. Lightweight — one line per item. Metadata tokens are inline. |
| **Table** | Rich information per item (status, owner, priority, due, custom fields) | Compact for flat task lists with multiple columns. Each row is a **leaf task** — no milestone/task distinction. Hierarchy can be approximated by adding a `Level` or `Phase` column for the human reader, but trawl still treats every row as a flat leaf. |
| **Mixed** | Both hierarchical structure and dense per-group metadata | Use checkboxes for the hierarchy, tables for groups that need per-item columns. Subsection headings between groups are structural. |

**Structure** — how items are grouped into a tree (group nodes), on top of any format:

| Structure | Best for | Trade-offs |
|-----------|----------|------------|
| **Indentation** (checkbox-native) | Milestone → task within checkbox format | 2 spaces per level; the baseline hierarchy for checkbox lists (see [Checkbox items](#checkbox-items)). |
| **Subsection headings** (`###`) | Grouping large trackers into named phases | `### Title` becomes a group node (no checkbox) that owns the items beneath it. Heading level relative to the section determines nesting; a heading resets indentation context. |
| **Plain-bullet groups** | Group nodes without a checkbox state | `- Group` followed by indented children becomes a group node. Plain bullets without children are ignored — they stay as context notes. |
| **Cross-document reference** | Multi-file objectives (epic across planning docs, learning track across chapter notes) | `[[target]]` or `[display](target)` pulls in another doc's tracker as a subtree. See [Cross-document references](#cross-document-references). |

A heading **inside** the section is **structural** — it becomes a group
node whose children are the items beneath it. The heading level relative
to the section determines nesting: `####` inside `###` nests one level
deeper. A heading **closes any open checkbox indentation context**, so
items after it become children of the heading, not of the prior checkbox
parent.

**Guideline**:

- **Format**: start with checkboxes. Switch to tables (or mixed) when each
  item carries more than one tracked attribute beyond the task name itself.
- **Structure**: start flat. Add subsection headings when the tracker grows
  past a dozen items; use cross-document references when an objective spans
  multiple files.

### Checkbox items

```
- [x] a completed item
- [ ] a pending item
```

| Aspect | Rule |
|--------|------|
| List markers | `-`, `*`, or `+` (markdown-conventional: `-`) |
| Checked characters | `x`, `X`, or `✓` mean done; a space means not done |
| Indentation | **2 spaces per level** (the Markdown standard). Indentation defines hierarchy: items with indented children are *milestones*; items without children are *leaf tasks*. Deeper nesting creates sub-tasks naturally. |

```markdown
## GOAL TRACKER

- [x] Week 1: Introduction
  - [x] Lecture 1: How an LLM is made
  - [x] Assignment 1: Basic prompting
  - [ ] Reading: Prompt Engineering Guide !low
    - [ ] Skim the introduction
    - [ ] Take detailed notes on sections 2-4
- [ ] Week 2: Power Prompting
  - [ ] Lecture 3 !high @yiqun
  - [ ] Assignment 2
- [ ] Buy textbook
```

### Tables

Tables provide a compact, columnar format for flat task lists with
per-task metadata. Each row is a **leaf task** — no milestone/task
distinction.

**Malformed and skipped tables are not dropped silently.** If a table is
missing its separator row, or has no column that maps to task, trawl
emits a `⚠` warning marker at that position rather than ignoring the
block. The marker appears in the TUI goals view and in the
`--no-tui` `warnings:` section (alongside broken-reference and cycle
markers), so a forgotten separator or an unregistered task column never
causes quiet data loss.

#### Column detection

Trawl maps columns by scanning header names with **case-insensitive
substring matching** against configurable keyword lists:

| Field | Default keywords | Required? |
|-------|-----------------|-----------|
| Task description | `task`, `item`, `name`, `todo`, `work` | **Yes** — at least one column must map to "task", otherwise the table is replaced by a `⚠ (table skipped: no task column)` warning marker |
| Completion state | `state`, `status`, `done`, `progress`, `check` | Optional — if no state column is found, all rows default to not-done |
| Owner | `owner`, `assignee`, `who` | Optional |
| Priority | `priority`, `pri` | Optional |
| Tag | `tag`, `category`, `label` | Optional |
| Due date | `due`, `deadline`, `target` | Optional |
| *(anything else)* | Custom field: key = header text, value = cell content | — |

> **Substring matching**: "Chapter" does not contain any task keyword
> (`task`, `item`, `name`, `todo`, `work`), so a `| Chapter |` column
> does not map to "task" unless you add `"chapter"` to
> `headers.task` in `.trawl.toml`. Similarly, adding `"chapter"` means
> any header *containing* "chapter" as a substring (e.g., "Chapter
> Notes") would also match — choose keywords unlikely to cause false
> positives.

#### State column

The state column indicates whether each task is done or not-done. How
trawl interprets the cell value is covered in the **Interpretation**
section below; syntactically, the state column is any column whose
header contains a state keyword (`state`, `status`, `done`, `progress`,
`check`).

#### Table example

```markdown
## GOAL TRACKER

| Task | State | Priority | Assignee | Estimate |
|------|-------|----------|----------|----------|
| OAuth2 flow | TODO | high | alice | 2d |
| Token refresh | | med | bob | 1d |
| Integration tests | | low | | 3d |
| Deprecate old auth | done | med | alice | 1d |
```

Parsed as:

```
  [ ] OAuth2 flow          priority: high   owner: alice   estimate: 2d
  [ ] Token refresh        priority: med    owner: bob     estimate: 1d
  [ ] Integration tests    priority: low                   estimate: 3d
  [x] Deprecate old auth   priority: med    owner: alice   estimate: 1d
```

### Mixed format

Checkbox lists, group nodes, references, and tables can be freely mixed
within the same section. `###` headings are **structural** — they group
the items beneath them:

```markdown
## GOAL TRACKER

### Fundamentals

- [x] Chapter 1: Introduction
- [x] Chapter 2: Language Basics
- [ ] Chapter 3: Data Structures !high

### Practice projects

| Done | Project | Notes |
|------|---------|-------|
| | text editor | part 2 |
| x | file converter | part 5 |
```

The parser produces a goal with two top-level group nodes (Fundamentals,
Practice projects), each owning its three checkbox children or two table
rows.

### Cross-document references

A `[[target]]` or `[display](target)` line inside a tracker pulls in
another doc's tracker as a subtree. The reference line becomes the
subtree root; the referenced doc's items become its children.

```markdown
<!-- ml/llm/README.md -->
# ML Learning Track

## GOAL TRACKER

- [x] Set up environment
- [ ] [[foundations/README]]
- [ ] [[advanced/README]]
```

The reference resolves relative to the referencing doc's directory
(`ml/llm/foundations/README.md` here). Optional `#anchor` suffixes are
stripped before resolution — whole-doc inlining is performed today.

Resolution keys on **file path**, not section name: the target just needs
*some* recognized tracker section. Renaming a tracker section never breaks
inbound references; moving or renaming the target *file* does.

For a reference to resolve, the **target file** must independently pass
every scan rule that any other scanned file does — trawl does not relax
detection for referenced docs:

- **Git-tracked** (unless `only_tracked = false`). A gitignored or
  untracked target is invisible to trawl.
- **Not in a dot-directory or dotfile** (unless `scan_hidden = true`),
  and not matched by an `exclude` glob.
- **Within `max_file_size`** (default `1MB`).
- **Contains its own goal-tracker section** — a heading matching a
  `goal_section_names` entry (`GOAL TRACKER` / `TODO` by default,
  case-insensitive exact match). Without it the reference resolves to a
  `⚠ (no goal tracker: …)` marker even though the file was scanned.

These are the same rules listed under *Detection* above; they apply to
the target independently of the referencing doc. A reference that
renders as `⚠ (not found: …)` almost always means the target failed one
of them — check `git ls-files <target>` and the exclude/hidden settings
before assuming a syntax problem.

| Outcome | Trigger | What the user sees |
|---------|---------|---------------------|
| **Resolved** | Target has a goal tracker | Imported subtree under the reference line. Text comes from the target's H1 (wikilink, falling back to the filename if there is no H1) or the link display text (markdown link). |
| **Broken: not found** | Target file is not in the scan set | `⚠ (not found: target)` leaf — no subtree. |
| **Broken: no goal tracker** | Target was scanned but has no tracker | `⚠ (no goal tracker: target)` leaf — no subtree. |
| **Cycle** | Target is already on the expansion chain (A → B → A) | `↻ (cycle: a → b → a)` leaf — expansion stops. |

A referenced tracker becomes a **subtree** of its parent and no longer
appears as a separate top-level goal — each tracker shows exactly once in
the dashboard (nested under the goal that references it). A goal may be
referenced from multiple parents (a diamond); each parent gets its own
deep-cloned copy, and the referenced tracker is absent from the top-level
list. Mutually-cyclic goals (A → B → A, with no outside root) are an
exception: having no root to hang from, they remain top-level and the
cycle is shown as a `↻` marker.

---

## Metadata

How to annotate items with structured data.

### Token syntax

Metadata tokens are optional, space-separated prefixes embedded anywhere
in the item text. Trawl extracts them and removes them from the
description — the remaining text becomes the task description.

| Field | Default prefix | Example |
|-------|---------------|---------|
| Owner | `@` | `@yiqun` |
| Tag | `#` | `#security` (multiple allowed: `#security #arch`) |
| Priority | `!` | `!high` — values: `high`, `med`/`medium`, `low` (case-insensitive). Unrecognized values stored as-is. |
| Due | `~` | `~2025-12-01` (YYYY-MM-DD format) |

**Parsing rules**:

- A token **starts** at a prefix character preceded by whitespace or
  start-of-text (prevents false positives like `file#tag`).
- A token **ends** at the next whitespace or end of string.
- Prefixes not in the configured token set are left in the description
  as plain text.

```
Input:  "Lecture 3: Power Prompting #security @yiqun !high ~2025-02-15"
Parsed:
  description = "Lecture 3: Power Prompting"
  tags        = ["security"]
  owner       = "yiqun"
  priority    = high
  due         = 2025-02-15
```

> Note: `#security` is a **tag** (prefix `#`), while `!high` is a
> **priority** (prefix `!`). Tags are free-form labels; priorities are
> a fixed enum. Never use `#high` for priority — it would be parsed as
> a tag named "high", not a priority level.

### Column override rule

In table rows, column-based values (from recognized headers like
"Assignee" → owner) **override** inline tokens for the same field. If a
row has `@bob` in the task cell AND "alice" in the Assignee column, the
owner is "alice".

**Use bare values in cells, not token-prefixed ones.** The column header
already identifies the field, so write `high` (not `!high`), `alice` (not
`@alice`), `2025-12-01` (not `~2025-12-01`). A cell value is parsed
literally: `!high` in a Priority cell does not match the `high`/`med`/`low`
enum and falls through to the unrecognized-value bucket, defeating
priority filtering and sorting. The `!`/`@`/`#`/`~` prefixes are for
**inline use in free text only** (checkbox items, bullet text).

### Custom tokens

Token prefixes are configurable via `[tokens]` in `.trawl.toml`. Add
domain-specific metadata types:

```toml
[tokens]
effort = "%"     # e.g., %2h for estimated effort
```

Custom tokens follow the same parsing rules: start at prefix after
whitespace, end at next whitespace, extracted and stored as-is.

---

## Interpretation

How trawl computes progress and status from parsed content.

### Done detection

Trawl has a **binary** model: every item is either done or not-done.
There are no intermediate states like "in progress" or "blocked".

**For checkbox items**: `- [x]` = done, `- [ ]` = not-done. The checked
character (`x`, `X`, `✓`) determines the state directly.

**For table state columns**: completion is determined by the done
heuristic:

```
not-done = cell is empty OR cell contains "TODO" (case-insensitive)
done     = everything else
```

| Cell value | Done? | Reason |
|-----------|-------|--------|
| *(empty)* | No | Empty |
| `TODO` / `todo` | No | Contains `TODO` |
| `done` | Yes | Non-empty, no `TODO` |
| `x` / `✓` | Yes | Non-empty, no `TODO` |
| `shipped` / `skipped` / `wontfix` | Yes | Non-empty, no `TODO` |
| `IN PROGRESS` | Yes | Non-empty, no `TODO` |

**Practical implication**: "IN PROGRESS", "wontfix", and "skipped" all
count as done. To keep an item as not-done in a table, leave the state
cell **empty** or write "TODO". To preserve intermediate status text,
put it in a custom "Notes" column rather than the state column.

### Progress (leaf ratio)

All levels use **leaf ratio** — the ratio of done leaves to total leaves
within the scope. A node counts as a **leaf** when it has no children, a
checkbox state (checkbox tasks and table rows), and is **not a dead
reference**. A *dead reference* — one that resolved to Broken or Cycle —
carries no completable work, so it never counts, **regardless of whether
the user wrote a checkbox on the reference line** (`- [ ] [[missing]]` is
invisible to progress, just like a standalone broken `[[missing]]`).
Empty group nodes (e.g. an isolated `###` heading) likewise do not count:

| Scope | Formula |
|-------|---------|
| Leaf (checkbox/table row, no children) | `1` if checked, `0` if not |
| Milestone (checkbox, has children) | `count(done leaves in subtree) / count(all leaves in subtree)` |
| Group node (no checkbox, has children) | same formula as milestone |
| Dead reference / empty heading | does not count — invisible to progress |
| Goal | `count(done leaves) / count(all leaves)` |

**Zero leaves**: if a scope contains no leaves (empty section, milestones
without tasks beneath them, only group nodes, or only dead references),
progress is `0%` — no division by zero is performed.

### Status

| Status | Condition |
|--------|-----------|
| `completed` | Progress = 100% |
| `active` | 0% < progress < 100% |
| `planned` | Progress = 0% (including zero-leaf goals) |

### Milestone checkbox independence

A milestone's own checkbox state (`[x]` or `[ ]`) is **independent** of
its children's leaf ratio. The user controls it manually. The TUI
shows both: the checkbox state and a direct-children count (e.g.,
`[x] Week 1  2/3`).

### Table row semantics

Every table row is a **leaf task** — flat, no nesting. Tables cannot
express milestones. If hierarchical grouping is needed, use checkboxes
for the tree and tables for per-group metadata, or add a `Level`/`Phase`
column for visual organization (trawl ignores it for hierarchy
computation).

---

## Configuration

When trawl's defaults don't match your content.

### Default config values

```toml
[scan]
keywords = ["TODO", "FIXME", "HACK", "XXX", "BUG"]
keyword_case_sensitive = false
goal_section_names = ["GOAL TRACKER", "TODO"]
include = []                      # empty = all file types
exclude = ["target/", "node_modules/", ".git/"]   # built-in; project config merges (union)
max_file_size = "1MB"
scan_hidden = false
only_tracked = true

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
show_git_blame = true
context_lines = 2
auto_expand_priority = "high"
stale_threshold_days = 365
```

### .trawl.toml format

A `.trawl.toml` file in the repository root provides per-project
overrides. Configuration is layered — later sources override earlier:

```
built-in defaults  →  ~/.config/trawl/config.toml  →  <repo>/.trawl.toml  →  CLI flags
```

**Merge semantics**:

- Scalars (`max_file_size`, `only_tracked`, etc.) are **replaced** when
  a layer provides them.
- `exclude` and `include` **merge** (union, de-duplicated) across all
  layers with the built-in defaults. A project that adds `exclude =
  ["docs/"]` still skips `target/` without re-listing it.
- `[tokens]` and `[headers]` **merge entry-by-entry**. Adding
  `effort = "%"` retains all default tokens; adding `task = ["chapter"]`
  to `[headers]` extends the default task keyword list.

### When config is needed

| Reason | Frequency | When to add |
|--------|-----------|-------------|
| **Header keyword registration** | Routine | A table column intended as a standard field (task, state, owner, priority, tag, due) uses a header keyword not in the default lists. Domain-specific vocabulary ("Chapter", "Paper", "Recipe") is legitimate — trawl should adapt via config, not require column renaming. |
| **Section name registration** | Last resort | A non-standard heading name needs recognition. Prefer renaming the heading to match a default (`GOAL TRACKER` or `TODO`) unless the name carries domain meaning the user wants preserved. |
| **Exclude paths** | When needed | Noise directories (`.agents/`, `vendor/`, test fixtures) contain marker patterns that interfere with scanning. |

Omit `.trawl.toml` entirely when none of these conditions apply.

Example — registering a domain-specific column header:

```toml
# .trawl.toml
[headers]
task = ["chapter"]    # extends the default list; "Chapter" columns now map to task
```

---

## Verification

How to confirm your goal trackers are trawl-compatible.

### Compatibility checklist

Ordered by pipeline stage — check each before moving to the next:

**Detection**:

- [ ] The file is git-tracked (`git ls-files <path>` shows it)
- [ ] The file is not in a dot-directory (unless `scan_hidden = true`)
- [ ] The file does not exceed `max_file_size`
- [ ] The file is not in an excluded directory

**Section**:

- [ ] A heading in the file matches a `goal_section_names` entry (case-insensitive exact match)
- [ ] No higher-level heading sits between the section and its items
- [ ] Only one tracker section per file (first match wins)

**Items**:

- [ ] Items use `- [ ]` / `- [x]` (or `*`, `+`; checked char is `x`/`X`/`✓`)
- [ ] Nesting uses 2-space indentation
- [ ] Subsection headings (`###`) inside the section become group nodes — verify nesting by relative heading level
- [ ] Plain bullets (`- text`) with children become group nodes; without children they are ignored
- [ ] Tables have a header row, a separator row (`|---|`), and at least one column whose header contains a task keyword
- [ ] Table state cells follow the done heuristic (empty or "TODO" = not-done; anything else = done)

**References** (if used):

- [ ] Reference lines are line-as-reference (`- [ ] [[target]]` or standalone `[[target]]`) — embedded refs like `"see [[x]] for details"` stay literal
- [ ] Targets resolve relative to the referencing doc's directory
- [ ] Targets end in `.md` (or omit the extension — trawl appends it)
- [ ] **Target preconditions** — the referenced file, independently of the referencing doc, must satisfy:
    - [ ] Git-tracked (`git ls-files <target>` shows it), unless `only_tracked = false`
    - [ ] Not in a dot-directory or dotfile (unless `scan_hidden = true`), and not matched by an `exclude` glob
    - [ ] Within `max_file_size`
    - [ ] Contains a goal-tracker section heading (`GOAL TRACKER` / `TODO` by default) — otherwise the reference becomes a `⚠ (no goal tracker: …)` marker
- [ ] No `⚠` markers (broken refs, malformed/skipped tables) and no `↻` cycles — all are listed in the `--no-tui` `warnings:` section

**Metadata**:

- [ ] Tokens are space-delimited (`!high @owner #tag ~date`)
- [ ] `#tag` is for labels, `!priority` is for triage levels — never use `#high` for priority
- [ ] Table cells use **bare** values (`high`, not `!high`) — token prefixes are inline-only

**Config**:

- [ ] Every table column serving as a standard field contains a keyword from the appropriate default list (or is registered in `.trawl.toml`)
- [ ] The file has a `# H1` title — otherwise trawl falls back to the filename

**Verification**:

- [ ] Run `trawl --path <dir> --no-tui` and confirm every expected goal appears with correct progress/status
- [ ] The `warnings:` section is empty — a non-empty list flags malformed/skipped tables, broken references, or cycles that would otherwise be silent

### Diagnosing missing goals

If a goal does not appear in `trawl --no-tui` output, follow this
decision tree:

```
Goal not visible?
│
├─ Is the file git-tracked?
│  └─ No → git add the file, or set only_tracked = false
│
├─ Is the file in a dot-directory?
│  └─ Yes → set scan_hidden = true, or move the file
│
├─ Is the file excluded?
│  └─ Yes → remove the exclude pattern, or set include to whitelist it
│
├─ Does the file contain a matching section heading?
│  └─ No → add a ## GOAL TRACKER or ## TODO section
│  └─ Heading present but wrong name → check goal_section_names config
│
├─ Does the section contain checkbox items or tables?
│  └─ No items → goal appears as "planned" (0%)
│  └─ Tables only → check that at least one column maps to "task"
│     └─ No task column → add keyword to headers.task in .trawl.toml
│     └─ Or rename the column header to use a default task keyword
│
└─ Check trawl --no-tui output again
```
