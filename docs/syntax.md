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

A goal tracker is a markdown section whose heading matches one of the
configured section names (case-insensitive, exact match).

**Detection rules:**

1. Scan markdown headings for a case-insensitive match against any name in
   `goal_section_names` (configurable; defaults are `GOAL TRACKER` and
   `TODO`).
2. The heading may be any level (`#`, `##`, `###`, etc.).
3. The section's content extends until the next heading of the **same or
   higher** level (or end of file).
4. Only the first matching section in a file is parsed.

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

Within the GOAL TRACKER section, the parser recognizes **four** element
types:

| Element | Syntax | Parsed as |
|---------|--------|-----------|
| Checkbox item | `- [x]` or `- [ ]` (at any indent) | Checkbox node in the tree (task or milestone, depending on children) |
| Plain bullet | `- text` (no checkbox) | **Group node** if it has indented children; ignored otherwise (preserves context-note behavior) |
| Subsection heading | `### title` (any level deeper than the section) | **Group node**; items beneath it become its children |
| Reference | `[[target]]` or `[text](target)` on its own or inside a checkbox/bullet | Node carrying a `Reference`; resolved into a subtree of the target doc in Pass 2 |
| Table | `\| ... \|` with separator row | Task table (see Table Format) |

**Everything else is ignored** — including paragraphs, images, code blocks,
and blank lines. Headings at the **same or higher level** than the section
end the section (and are therefore outside it).

### Checkbox Tree (List Format)

#### Hierarchy from Indentation

The checkbox tree *is* the hierarchy. There are no separate grammar
rules for milestones vs tasks — the distinction comes from tree
position:

- A checkbox item **with children** (more-indented checkboxes below it)
  = **milestone**
- A checkbox item **without children** = **task**
- Deeper nesting creates sub-tasks naturally

Plain bullets and subsection headings (see [Group Nodes](#group-nodes))
introduce additional internal node types alongside checkbox milestones.

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
leaf tasks within the scope. A "leaf" is a node with no children **and**
a checkbox state; **group leaves** (empty subsections, broken references,
cycle markers) do **not** count toward total or done:

```
task (no children):          progress = 1 if [x], else 0
milestone (has children):    progress = count(done checkbox leaves in subtree)
                                       / count(all checkbox leaves in subtree)
group node (has children):   same formula as milestone
group leaf (no children):    does not count — invisible to progress
goal:                        progress = count(done checkbox leaves) / count(all checkbox leaves)
```

**Zero checkbox leaves:** if a scope contains no checkbox leaves (an empty
section, only milestones with no tasks beneath them, or only group nodes),
progress is `0%` and no division is performed. This avoids division by
zero; the scope reports status `planned` (see Derived Fields).

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

Checkbox lists, group nodes, references, and tables can be freely mixed
within the same GOAL TRACKER section. Headings (`###`, etc.) **inside**
the section are structural — they become group nodes whose children are
the items beneath them. Headings at the **same or higher** level than
the section end the section.

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

The `### Foundations` and `### Practice Projects` headings become group
nodes — the parser sees a goal with two top-level items, each owning its
checkbox children or table rows.

### Group Nodes

A **group node** is a named container without a checkbox. It carries a
title and may have child items, but it has no `[ ]` / `[x]` state of
its own. Group nodes come from three sources:

| Source | Syntax | Notes |
|--------|--------|-------|
| Subsection heading | `### Title` (level deeper than section) | Always retained, even when a heading immediately follows (a planned placeholder). |
| Plain bullet with children | `- Group` followed by indented items | Becomes a group node; nested indentation defines its subtree. |
| Plain bullet without children | `- just a note` | **Ignored** — preserved context-note behavior. |
| Reference line | `[[target]]` or `[Link](target)` | Always a group node; resolved into a subtree in Pass 2. |

**Subsection nesting.** A heading at relative level *k* (heading level
minus section level) becomes a child of the most recent heading at level
< *k*. A `####` inside a `###` nests one level deeper. A `###` after a
`####` pops back to top-level.

**Indent reset.** A heading **closes any open checkbox indentation
context**. Items after the heading form a fresh indentation tree rooted
at that heading — they are not children of the prior checkbox parent.

```markdown
## GOAL TRACKER

- [ ] Pre-section task          ← top-level (root of goal)

### Phase 1                     ← group node (top-level)

- [ ] Task A                    ← child of "Phase 1", not "Pre-section task"
  - [ ] Sub-task                ← child of "Task A"

#### Sub-area                   ← group node, child of "Phase 1"

- [ ] Deep task                 ← child of "Sub-area"

### Phase 2                     ← group node (top-level, sibling of Phase 1)
```

Parsed tree:

```
Goal
├── [ ] Pre-section task
├── Phase 1 (group)
│   ├── [ ] Task A
│   │   └── [ ] Sub-task
│   └── Sub-area (group)
│       └── [ ] Deep task
└── Phase 2 (group)
```

A group node carries its own metadata tokens (e.g., `### Phase 1 !high`)
which apply to the group itself, not its children.

### Cross-Document References

A **reference** lets one goal tracker pull in another doc's tracker as
a subtree. The reference line becomes the subtree root; the referenced
doc's items become its children.

#### Syntax

Two reference forms are recognized, matching common PKM conventions:

| Form | Syntax | Resolves to |
|------|--------|-------------|
| Wikilink | `[[target]]` | Subtree rooted at the line; node text is filled from the target's H1 title. |
| Markdown link | `[display](target)` | Subtree rooted at the line; node text is the `display` text. |

A reference may appear:

- **Inside a checkbox item**: `- [ ] [[target]]` — the checkbox state
  applies to the imported subtree as a milestone state.
- **Inside a plain bullet**: `- [[target]]` — becomes a group node.
- **As a standalone line**: `[[target]]` (no bullet) — becomes a group
  node at the current indentation level.

Embedded references (e.g., `- [ ] see [[x]] for details`) are **not**
treated as structural — the link stays as literal text. Only
line-as-reference is meaningful.

#### Path resolution

Reference targets resolve **relative to the referencing doc's
directory**, matching how markdown links work in renderers. A reference
in `ml/llm/cs146s/README.md` writing `[[overview/plan]]` resolves to
`ml/llm/cs146s/overview/plan.md`.

- An optional `#anchor` may be appended (`[[doc#section]]`); it is
  stripped before resolution. Whole-doc inlining is performed today;
  section-level resolution is future work.
- If the target has no extension, `.md` is appended automatically.
- Path keys normalize backslashes to forward slashes for cross-platform
  comparison.

#### Resolution outcomes

After parsing all goals (Pass 1), a second pass walks each tree and
converts each `Pending` reference into one of:

| Outcome | Condition | Display |
|---------|-----------|---------|
| **Resolved** | Target path is in the goal map | The target's items become children of the referencing node. |
| **Broken: NotFound** | Target path is not in the scan set | Rendered with `⚠ (not found: target)`; no children. |
| **Broken: NoGoalTracker** | Target was scanned but has no tracker | Rendered with `⚠ (no goal tracker: target)`; no children. |
| **Cycle** | Target is already on the active expansion chain | Rendered with `↻ (cycle: a → b → a)`; expansion stops. |

A goal may be referenced from multiple parents (a diamond); each
expansion gets its own deep-cloned copy. Every goal tracker is also
shown top-level in the dashboard — references add additional nested
views, they never replace the source.

#### Example

```markdown
<!-- ml/llm/README.md -->
# ML Learning Track

## GOAL TRACKER

- [x] Set up environment
- [ ] [[foundations/README]]
- [ ] [[advanced/README]]
```

```markdown
<!-- ml/llm/foundations/README.md -->
# Foundations

## GOAL TRACKER

- [x] Transformer architecture
- [ ] Attention mechanics
```

After resolution, the `ml/llm/README.md` goal contains:

```
ML Learning Track
├── [x] Set up environment
├── [ ] Foundations               ← from [[foundations/README]]
│   ├── [x] Transformer architecture
│   └── [ ] Attention mechanics
└── [ ] Advanced                  ← from [[advanced/README]]
    └── …
```

### Derived Fields

| Field | Derivation | Example |
|-------|-----------|---------|
| Title | First `#` H1 in the file. Fallback: filename without extension. | "Complete CS146s-2025" |
| Progress | `count(done checkbox leaves) / count(all checkbox leaves) * 100`; `0%` when there are no checkbox leaves. Group leaves do not count. | 40% |
| Status | `completed` if progress = 100%. `active` if 0 < progress < 100%. `planned` if progress = 0% (including goals with no checkbox leaves). | active |

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
| Keyword | Yes | One of the configured keywords (default: `TODO`, `FIXME`, `HACK`, `XXX`, `BUG`) |
| Scope | No | `(name)` — module or area grouping |
| Separator | Yes | `:` or space between keyword/scope and description |
| Description | Yes | Free text (everything before first metadata token) |
| Metadata tokens | No | Same `@owner`, `#tag`, `!priority`, `~due` as goal tasks |

### Grammar

```
inline_task      ::= keyword scope? sep text

keyword          ::= configured keyword
                   (default: "TODO" | "FIXME" | "HACK" | "XXX" | "BUG",
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

### String-literal and Code-span Skipping

A keyword that appears inside a double-quoted region or a backtick-delimited
inline code span is treated as **data**, not a task annotation, and is not
reported. This avoids false positives where the keyword is a string literal
(`"TODO".into()`), a test fixture, or prose that merely mentions the keyword.

It is enabled by default; set `scan.skip_quoted_keywords = false` to restore
raw first-match behavior.

```
"TODO".into()       →  skipped (string literal)
see `FIXME` here    →  skipped (code span)
// TODO: real task  →  reported (the annotation itself)
"x" // TODO: after  →  reported (keyword is outside the string)
```

Notes:
- Single quotes are **not** delimiters. Apostrophes in prose (`don't`) would
  otherwise open a string that never closes and hide later keywords on the
  line; keywords in single-quoted strings (e.g. Python `'TODO'`) are still
  reported.
- A backslash escapes the next byte, so `"a\"b"` closes correctly.
- The check is per-line and language-agnostic; it does not track multi-line
  string or block-comment state.

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

- **Frontmatter / YAML metadata**: no `---` blocks. Goal metadata is
  derived from the markdown structure itself.
- **Lifecycle states** (`in-progress`, `blocked`, `wontfix`): inline
  tasks are binary (exist or removed). Goal tasks use the done heuristic.
- **Embedded references as structural**: `see [[x]] for details` inside a
  line is treated as literal text. Only line-as-reference is structural.
- **Section-level reference resolution**: `[[doc#section]]` resolves to
  the whole doc today; targeting a specific subsection is future work.
- **Nested tasks within a single comment block**: each line is parsed
  independently.
- **Tasks in compiled output** (`.o`, `.exe`, `.class`): binary files are
  skipped entirely.
