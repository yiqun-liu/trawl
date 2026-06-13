# TRAWL — Syntax Specification

This document defines the formal syntax for the two annotation types
TRAWL scans for: **goal trackers** and **inline tasks**.
Both share a common metadata token system.

## Shared Metadata Tokens

Both goal tracker tasks and inline tasks support optional inline metadata.
Tokens are identified by configurable prefix characters and may appear
anywhere in the item text.

### Token Definitions

| Token | Default prefix | Example | Field |
|-------|---------------|---------|-------|
| Owner | `@` | `@yiqun` | Who owns this item |
| Tag | `#` | `#security` | Category or label |
| Priority | `!` | `!high` | Triage level |
| Due | `~` | `~2025-12-01` | Deadline (YYYY-MM-DD) |

**Priority values**: `high`, `med` (or `medium`), `low` (case-insensitive).
Unrecognized values are stored as-is.

### Parsing Rules

- Tokens are extracted from text by scanning for prefix characters.
- A token **starts** at a prefix character preceded by whitespace or
  start-of-text. This prevents false positives like `file#tag` or
  mid-word matches.
- A token **ends** at the next whitespace or end of string.
- Extracted tokens are removed from the text. The remaining text
  (including gaps between removed tokens) becomes the description,
  trimmed of excess whitespace.
- Multiple tokens of the same type are allowed (e.g., `#security #arch`
  produces two tags).
- Prefixes not in the configured token set are left in the description
  as plain text.

**Example:**

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
> a fixed enum (`high`/`med`/`low`). Never use `#high` for priority —
> it would be parsed as a tag named "high", not a priority level.

### Configurability

Prefixes are defined in the config file:

```toml
[tokens]
owner = "@"
tag = "#"
priority = "!"
due = "~"
# Custom tokens can be added:
effort = "%"     # e.g., %2h for estimated effort
```

---

## Goal Tracker Syntax

### Section Detection

A goal tracker is a markdown section named `GOAL TRACKER`.

**Detection rules:**

1. Scan markdown headings for one matching `GOAL TRACKER` (case-insensitive).
2. The heading may be any level (`#`, `##`, `###`, etc.).
3. The section's content extends until the next heading of the **same or
   higher** level (or end of file).
4. Only the first `GOAL TRACKER` section in a file is parsed.
5. Configurable: `goal_section_names` in config (default: `["GOAL TRACKER"]`).

**Example — section boundaries:**

```markdown
# Complete CS146s-2025

Some intro text.                    ← ignored

## GOAL TRACKER                     ← section starts

- [x] Week 1: Introduction          ← parsed (milestone)
  - [x] Lecture 1                   ← parsed (task)

## References                       ← section ends (same level as ##)
- [Stanford CS146s](...)            ← ignored
```

### Content Within the Section

Within the GOAL TRACKER section, the parser recognizes **only** two
element types:

| Element | Syntax | Parsed as |
|---------|--------|-----------|
| Checkbox item | `- [x]` or `- [ ]` (at any indent) | Task node in the checkbox tree |
| Table | `\| ... \|` with separator row | Task table (see Table Format) |

**Everything else is ignored** — including headings (`###`, `####`),
paragraphs, plain bullets, images, code blocks, and blank lines. This
lets users mix context notes and visual formatting with structured
tracking freely.

### Checkbox Tree (List Format)

#### Hierarchy from Indentation

The checkbox tree *is* the hierarchy. There are no separate grammar
rules for milestones vs tasks — the distinction comes from tree
position:

- A checkbox item **with children** (more-indented checkboxes below it)
  = **milestone**
- A checkbox item **without children** = **task**
- Deeper nesting creates sub-tasks naturally

`###` and other headings within the section are visual formatting only —
ignored by the parser.

#### Grammar

```
list_content     ::= (checkbox_tree | table | ignored_line)*

checkbox_tree    ::= node+
node             ::= indent "- [" state "] " text eol
                     child*

indent           ::= "" | "  " | "    " ...    (2 spaces per level)
state            ::= "x" | "X" | " " | "✓"
text             ::= raw text on the line
                     (metadata tokens extracted post-parse —
                     see Shared Metadata Tokens)
child            ::= node with greater indent than parent

ignored_line     ::= any line not matching node
                   (headings, plain bullets, paragraphs, blank lines, etc.)
```

#### Progress Calculation

All levels use **leaf ratio** — the ratio of done leaf tasks to total
leaf tasks within the scope:

```
task (no children):     progress = 1 if [x], else 0
milestone (has children): progress = count(done leaf tasks in subtree) / count(all leaf tasks in subtree)
goal:                   progress = count(done leaf tasks) / count(all leaf tasks)
```

**Zero leaf tasks:** if a scope contains no leaf tasks (an empty section,
or only milestones with no tasks beneath them), progress is `0%` and no
division is performed. This avoids division by zero; the scope reports
status `planned` (see Derived Fields).

A milestone's own checkbox state is independent of its children — the
user controls it manually. The TUI shows both the checkbox state and
a direct-children count (e.g., `[x] Week 1  2/3`), where `2/3` means
"2 of 3 direct children are checked" — a quick visual indicator, not
the leaf-ratio progress percentage.

#### Example

```markdown
## GOAL TRACKER

- [x] Week 1: Introduction
  The first week covers LLM fundamentals.           ← ignored
  - [x] Lecture 1: How an LLM is made
  - [x] Assignment 1: Basic prompting
  - Note: skip the guest lecture                     ← ignored
  - [ ] Reading: Prompt Engineering Guide !low
    - [ ] Skim the introduction
    - [ ] Take detailed notes on sections 2-4
- [ ] Week 2: Power Prompting
  - [ ] Lecture 3 !high @yiqun
  - [ ] Assignment 2
- [ ] Buy textbook
```

#### Parsed Result

```
Goal: "Complete CS146s-2025"  (from H1)
  [x] Week 1: Introduction                           milestone  2/4 leaf
    [x] Lecture 1: How an LLM is made               task
    [x] Assignment 1: Basic prompting                task
    [ ] Reading: Prompt Engineering Guide            milestone  0/2 leaf
        priority: low
      [ ] Skim the introduction                     task
      [ ] Take detailed notes on sections 2-4       task
  [ ] Week 2: Power Prompting                        milestone  0/2 leaf
    [ ] Lecture 3                                    task
        priority: high
        owner: "yiqun"
    [ ] Assignment 2                                 task
  [ ] Buy textbook                                   task
Progress: 2/7 leaf tasks (29%)
Status: active
```

### Table Format

Tables provide a compact, columnar alternative to checkbox lists. They are
especially useful for flat task lists with per-task metadata.

Tables produce **flat task lists** — no milestone/task distinction. Each
row is a task.

#### Column Detection

The parser identifies columns by scanning header names (case-insensitive
substring match against configurable keyword lists):

| Header contains | Maps to | Default keywords |
|-----------------|---------|------------------|
| `task`, `item`, `name`, `todo`, `work` | Task description | (required — at least one column must match) |
| `state`, `status`, `done`, `progress`, `check` | Completion state | (optional) |
| `owner`, `assignee`, `who` | Owner metadata | (optional) |
| `priority`, `pri` | Priority metadata | (optional) |
| `tag`, `category`, `label` | Tag metadata | (optional) |
| `due`, `deadline`, `target` | Due date metadata | (optional) |
| *(anything else)* | Custom field (key = header, value = cell) | — |

**Fallbacks:**

- If no "task" column is found → table is skipped (cannot parse).
- If no "state" column is found → all tasks default to not-done.

#### Done Heuristic

For the state column, completion is determined by a loose heuristic:

```
done = NOT (cell.is_empty() OR cell.contains("TODO"))
```

This is **case-insensitive**. Any non-empty value that does not contain
"TODO" counts as done:

| Cell value | Done? | Reason |
|-----------|-------|--------|
| *(empty)* | No | Empty |
| `TODO` | No | Contains "TODO" |
| `todo` | No | Contains "TODO" (case-insensitive) |
| `done` | Yes | Non-empty, no "TODO" |
| `x` | Yes | Non-empty, no "TODO" |
| `✓` | Yes | Non-empty, no "TODO" |
| `shipped` | Yes | Non-empty, no "TODO" |
| `skipped` | Yes | Non-empty, no "TODO" |
| `wontfix` | Yes | Non-empty, no "TODO" |

This lets the user write whatever status convention feels natural — the
tool only cares whether the task is outstanding.

#### Metadata Tokens in Tables

Metadata tokens (`@owner`, `#tag`, `!priority`, `~due`) embedded in any
cell are extracted just as in list format. Column-based values (from
recognized headers) **override** inline tokens if both exist for the same
field.

#### Example

```markdown
## GOAL TRACKER

| Task | State | Priority | Assignee | Estimate |
|------|-------|----------|----------|----------|
| OAuth2 flow | TODO | high | alice | 2d |
| Token refresh | | med | bob | 1d |
| Integration tests | | low | | 3d |
| Deprecate old auth | done | med | alice | 1d |
```

#### Parsed Result

```
Goal: "Sprint 15: Auth Refactor"
  [ ] OAuth2 flow          priority: high   owner: alice   estimate: 2d
  [ ] Token refresh        priority: med    owner: bob     estimate: 1d
  [ ] Integration tests    priority: low                   estimate: 3d
  [x] Deprecate old auth   priority: med    owner: alice   estimate: 1d
Progress: 1/4 (25%)
Status: active
```

### Mixed Format

Checkbox lists and tables can be freely mixed within the same GOAL
TRACKER section. Headings (`###`, etc.) may be used for visual
separation but are ignored by the parser:

```markdown
## GOAL TRACKER

### Foundations

- [x] Chapter 1: Getting Started
- [x] Chapter 2: Guessing Game
- [ ] Chapter 4: Ownership !high

### Practice Projects

| Done | Project | Notes |
|------|---------|-------|
| | minigrep | ch 12 |
| x | web server | ch 20 |
```

The `###` headings are ignored — they serve as visual cues for the human
reader. The parser sees five tasks: three from the checkbox list and two
from the table.

### Derived Fields

| Field | Derivation | Example |
|-------|-----------|---------|
| Title | First `#` H1 in the file. Fallback: filename without extension. | "Complete CS146s-2025" |
| Progress | `count(done leaf tasks) / count(all leaf tasks) * 100`; `0%` when there are no leaf tasks. | 40% |
| Status | `completed` if progress = 100%. `active` if 0 < progress < 100%. `planned` if progress = 0% (including goals with no leaf tasks). | active |

No manual metadata fields are required. Everything is auto-derived.

---

## Inline Task Syntax

### Format

```
KEYWORD [(scope)][: or space] description metadata_token*
```

**Components:**

| Component | Required? | Description |
|-----------|-----------|-------------|
| Keyword | Yes | One of the configured keywords (default: `TODO`, `FIXME`, `HACK`, `XXX`, `BUG`, `NOTE`) |
| Scope | No | `(name)` — module or area grouping |
| Separator | Yes | `:` or space between keyword/scope and description |
| Description | Yes | Free text (everything before first metadata token) |
| Metadata tokens | No | Same `@owner`, `#tag`, `!priority`, `~due` as goal tasks |

### Grammar

```
inline_task      ::= keyword scope? sep text

keyword          ::= configured keyword
                   (default: "TODO" | "FIXME" | "HACK" | "XXX" | "BUG" | "NOTE",
                   case-insensitive by default)

scope            ::= "(" scope_name ")"
scope_name       ::= any text except ")"

sep              ::= ":" | " "

text             ::= the remainder of the line after keyword/scope/sep
                   (metadata tokens extracted post-parse —
                   see Shared Metadata Tokens)
```

### Keyword Semantics

Different keywords imply different default priorities:

| Keyword | Default priority | Meaning |
|---------|-----------------|---------|
| `TODO` | *(none)* | General pending work |
| `FIXME` | `high` | Known bug or broken code |
| `HACK` | `med` | Workaround that should be cleaned up |
| `XXX` | `med` | Warning or caution about the code |
| `BUG` | `high` | Confirmed bug |
| `NOTE` | *(none)* | Informational annotation |

Explicit `!priority` overrides the keyword default.

### Backward Compatibility

The parser accepts progressively less structured forms:

```
Fully structured:   TODO(auth): handle null user @yiqun #security !high ~2025-12-01
With scope:         TODO(auth): handle null user
With separator:     TODO: handle null user
Bare:               TODO handle null user
Minimal:            TODO
```

All five forms are valid. Missing fields default to `None`.

### Context Detection

The same `TODO` keyword can appear in many comment syntaxes. The parser
does **not** need to understand comment syntax — it simply looks for the
keyword pattern in any text line.

| Context | Example | Parsed correctly? |
|---------|---------|:-:|
| C line comment | `// TODO: fix this` | Yes |
| C block comment | `/* TODO: fix this */` | Yes |
| Shell comment | `# TODO: fix this` | Yes |
| Python comment | `# TODO(perf): optimize this !med` | Yes |
| Markdown inline | `TODO: add examples` | Yes |
| Markdown heading | `## TODO: refactor this section` | Yes |
| HTML comment | `<!-- TODO: fix this -->` | Yes |

### Markdown Heading Tasks

In markdown files, `## TODO` headings are a common pattern. The parser
handles these by stripping leading `#` characters before matching the
keyword:

```markdown
## TODO: refactor authentication
```

Parsed as: `keyword=TODO`, `description="refactor authentication"`.

### Examples

**C code:**
```c
/* TODO(auth): handle null user @yiqun #security !high ~2025-12-01 */
int get_user(const char *email) { ... }
```

**Shell script:**
```bash
# TODO(perf): optimize the scan loop !med
for file in $(find . -name '*.c'); do
```

**Rust code:**
```rust
// FIXME: this panics on empty input #bug !high
fn parse(input: &str) -> Result<...> { ... }
```

**Markdown notes:**
```markdown
TODO: add examples for cache types #arch
```

```markdown
## TODO: review this section
```

---

## What TRAWL Does Not Parse

The following are explicitly **out of scope**:

- **File references**: no `[[wikilink]]` or `→ file.md` syntax. Goals
  and inline tasks are independent peers, not linked by file references.
- **Frontmatter / YAML metadata**: no `---` blocks. Goal metadata is
  derived from the markdown structure itself.
- **Lifecycle states** (`in-progress`, `blocked`, `wontfix`): inline
  tasks are binary (exist or removed). Goal tasks use the done heuristic.
- **Nested tasks within a single comment block**: each line is parsed
  independently.
- **Tasks in compiled output** (`.o`, `.exe`, `.class`): binary files are
  skipped entirely.
