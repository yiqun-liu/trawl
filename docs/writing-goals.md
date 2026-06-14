# Writing trawl-Compatible Goal Trackers

A goal tracker is a standard markdown section that trawl auto-discovers.
You do not register or configure it — you write it once in any markdown
file and forget it. The files *are* the database.

## The heading trawl looks for

- `## GOAL TRACKER` — the canonical name.
- `## TODO` — also recognised by default.

The match is **exact** and **case-insensitive**. Only the first matching heading
in a file is parsed.

Everything outside the heading is free-form notes — ignored by trawl.

## Checkbox format

```
- [x] a completed item
- [ ] a pending item
```

Checked states: `x`, `X`, or `✓` all mean done. A space means not done.
Use `-`, `*`, or `+` as the list marker (markdown-conventional: `-`).

## Hierarchy is *indentation*

There are no special "milestone" or "task" markers. Nesting tells trawl
everything:

| You write | trawl sees |
|-----------|------------|
| `- [x] Week 1` with indented items *under* it | **milestone** |
| `- [x] Lecture 1` with nothing indented beneath | **leaf task** |
| Deeper nesting (4 spaces, 6 spaces, …) | **sub-tasks**, naturally |

Indent by **2 spaces per level** (the Markdown standard).

```markdown
## GOAL TRACKER

- [x] Week 1: Introduction
  - [x] Lecture 1: How an LLM is made
  - [x] Assignment 1: Basic prompting
  - [ ] Reading: Prompt Engineering Guide !low
- [ ] Week 2: Power Prompting
  - [ ] Lecture 3 !high @yiqun
  - [ ] Assignment 2
```

> Headings within the section (`###`, `####`, …) are visual formatting
> only — **ignored** by trawl. Plain paragraphs, blank lines, and bare
> bullets are also ignored. Only `- [ ]` lines and tables are parsed.

## Metadata tokens

Attach any of these to a checkbox line, space-separated anywhere in the
text. trawl strips them from the description and stores them typed.

| Token | Example | Meaning |
|-------|---------|---------|
| `@owner` | `@yiqun` | Who owns this item |
| `#tag` | `#security` | Category or label (multiple allowed) |
| `!priority` | `!high` | Triage level (`high`, `med`/`medium`, `low`) |
| `~due` | `~2025-12-01` | Deadline (YYYY-MM-DD) |

```markdown
- [ ] Lecture 3: Power Prompting !high @yiqun ~2025-12-15
```

Priorities `high`/`med`/`low` are colour-coded and triage-filtered.
Anything else (`!critical`, `!1`) is stored as-is.

## Table format (optional)

Checkbox lists and tables can be mixed freely. Tables produce flat task
lists (every row is a leaf). trawl maps columns by **header keyword**
(case-insensitive substring match):

| If the header cell contains … | It maps to |
|-------------------------------|------------|
| `task`, `item`, `name`, `todo`, `work` | Task description (required) |
| `state`, `status`, `done`, `progress`, `check` | Completion state |
| `owner`, `assignee`, `who` | Owner |
| `priority`, `pri` | Priority |
| `tag`, `category`, `label` | Tag |
| `due`, `deadline`, `target` | Due date |
| *(anything else)* | Custom field |

**At least one "task" column must be present;** otherwise the table is
skipped.

### How "done" is decided (the done heuristic)

trawl looks at the *state* column cell. The item is **not done** only when
the cell is *empty* or contains the substring `TODO` (case-insensitive).
Everything else counts as done.

| Cell value | Done? | Why |
|-----------|-------|-----|
| *(empty)* | No | Empty |
| `TODO` / `todo` | No | Contains `TODO` |
| `done` | Yes | Non-empty, no `TODO` |
| `x` | Yes | Non-empty, no `TODO` |
| `shipped` | Yes | Non-empty, no `TODO` |

```markdown
## GOAL TRACKER

| Task | State | Priority | Assignee |
|------|-------|----------|----------|
| OAuth2 flow | TODO | high | alice |
| Token refresh | done | med | bob |
| Integration tests | | low | |

### Foundations (ignored)

- [x] Chapter 1: Getting Started
- [ ] Chapter 4: Ownership !high
```

## What trawl derives

| Field | From | Example |
|-------|------|---------|
| **Title** | First `# H1` in the file (fallback: filename) | `"Complete CS146s-2025"` |
| **Progress** | Ratio of done leaves to all leaves | `60%` |
| **Status** | `completed` (100%), `active` (0-99%), `planned` (0%) | `active` |
| **Badge** | Super-directory of the file's owning directory | `ml/llm/` |

You do not set any of these fields — trawl computes them.

## Quick compatibility checklist

- [ ] The section heading contains `GOAL TRACKER` (any level)
- [ ] No higher-level heading sits between the section and its items
- [ ] Items use `- [ ]` / `- [x]` (or `*`, `+`; checked char is `x`/`X`/`✓`)
- [ ] Nesting uses **2-space indentation**
- [ ] Metadata tokens are space-delimited (`!high @owner #tag ~date`)
- [ ] Table has a header row, a separator row (`|----|`), and a recognisable task column
- [ ] The file has a title (`# Something`) on the first H1 — otherwise trawl falls back to the filename
- [ ] All leaf tasks are toggleable (`[ ]` → `[x]`); milestones fold by the leaf ratio beneath them

## Quick reference for AI agents

> You are writing markdown for the `trawl` TUI tool. Follow these rules:
>
> - Create a `## GOAL TRACKER` (or `## TODO`) section. Only the first match
>   in a file is parsed. The section ends at the next same-or-higher-level
>   heading.
> - Use `- [x]` (done) or `- [ ]` (pending) for every item. List markers
>   may be `-`, `*`, or `+`. `X` and `✓` also mean checked.
> - Indentation (2 spaces per level) defines the hierarchy: items with
>   children are *milestones*; items without children are *leaf tasks*.
> - `###` and other headings inside the section are decorative (ignored).
>   Plain paragraphs, blank lines, and bare bullets are also ignored.
> - Metadata tokens: `@owner`, `#tag`, `!high`/`!med`/`!low`, `~YYYY-MM-DD`.
>   Any token text; space-separated; appear anywhere on the line.
> - Tables are second-format citizens: `| Task | State | … |` followed by a
>   separator row with dashes. The "task" column is mandatory. State is
>   "done" unless empty or the cell contains `TODO`.
> - trawl derives the title from the first `# H1`, the progress from the
>   leaf checkbox ratio, and the badge from the file's super-directory.
> - ✅ Good: `- [ ] Refactor auth module !high @alice #security ~2025-06-01`
> - ❌ Avoid: bare bullets without `[ ]`, relying on `###` for structure,
>   mixing tab and space indentation, hiding tasks *above* the section
>   heading.
