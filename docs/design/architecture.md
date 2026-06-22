# TRAWL — Architecture

Implementation-level design. This document describes *how* trawl is built;
[`requirements.md`](../requirements.md) describes *what* it does and
[`syntax.md`](../syntax.md) describes *what* it parses.

## Scope of this revision

The initial implementation covers the **headless scan → parse pipeline**
(Phases 1–4 of the roadmap): scaffold, domain model, configuration, scanner,
and both parsers. The TUI, filtering/sorting, and git integration
are described at a high level.

```
             ┌─────────────────────────┐
             │  main.rs  (clap CLI)     │
             └────────────┬─────────────┘
                          │  Config
                          ▼
             ┌─────────────────────────┐
             │  scanner  (walk + read)  │
             └────────────┬─────────────┘
                          │  Vec<FileContents>
          ┌───────────────┴────────────────┐
          ▼                                ▼
   parser::inline                    parser::goal
   (keyword lines)                   (## GOAL TRACKER)
          │                                │
          └───────────────┬────────────────┘
                          ▼
                   ScanResult { goals, inline_tasks }
                          │
                          ▼
                  summary output (TUI later)
```

Everything flows through the shared domain model in `model.rs` and the
shared metadata extractor in `metadata.rs`.

## Module layout

```
src/
  main.rs        binary entry: clap CLI, logging init, calls lib::run
  lib.rs         crate root; re-exports public API for integration tests
  model.rs       domain types (Priority, Metadata, Span, InlineTask,
                 GoalItem, Goal, Status) + derived methods
  config.rs      layered Config (serde) + CLI flag merge
  metadata.rs    prefix-scan token extraction, shared by both parsers
  scanner/
    mod.rs       ScanOptions, scan() entry, ScanResult
    walker.rs    ignore-based recursive walk + 6-stage filter pipeline
    reader.rs    rayon parallel read + binary detection
  parser/
    mod.rs       ParserContext (keywords, token config), dispatch
    inline.rs    parse inline task lines
    goal.rs      parse ## GOAL TRACKER sections
tests/
  fixtures/
    inline/      one fixture per parsing form / comment context
    goal/        one fixture per goal-tracker scenario
```

`lib.rs` exposes the public surface so `tests/` can drive the scanners and
parsers as a library, independent of the CLI.

## Data model (`model.rs`)

The model is the single highest-leverage decision: both parsers and every
future view consume it. Two principles guide it:

1. **One item type per annotation kind.** Inline tasks and goal items are
   distinct because they answer different questions (binary existence vs.
   progress ratio).
2. **Unify checkbox nodes and table rows.** A table row is a leaf goal item
   whose `checked` state comes from the done-heuristic. One type serves
   both, so progress calculation and rendering share one code path.

```rust
/// A priority level. `None` (absence of a token) means untagged.
/// `Other` preserves unrecognized values verbatim ("stored as-is").
pub enum Priority { High, Med, Low, Other(String) }

/// Inline metadata extracted by prefix scan. Shared by both parsers.
pub struct Metadata {
    pub owner: Option<String>,                 // last value wins
    pub tags: Vec<String>,                     // accumulates
    pub priority: Option<Priority>,            // last value wins
    pub due: Option<chrono::NaiveDate>,        // last value wins
    pub custom: HashMap<String, Vec<String>>,  // any other configured tokens
}

/// 1-based source location.
pub struct Span { pub path: PathBuf, pub line: usize }

/// A TODO/FIXME/... marker discovered in a file.
pub struct InlineTask {
    pub keyword: String,
    pub scope: Option<String>,
    pub description: String,
    pub metadata: Metadata,
    pub span: Span,
    // Populated by blame enrichment when display.show_git_blame is true.
    pub blame_author: Option<String>,
    pub blame_date: Option<chrono::NaiveDateTime>,
    pub blame_commit: Option<String>,
}

/// One node in a goal-tracker tree.
///
/// `state` (a checkbox or a checkboxless group) is orthogonal to
/// `children` (leaf vs internal) and to `reference` (an optional
/// cross-document reference). Empty `children` => task or group leaf;
/// non-empty => milestone or group node. See `src/model.rs` for the
/// `NodeState` and `Reference` enum definitions.
pub struct GoalItem {
    pub text: String,
    pub state: NodeState,              // Checkbox { checked } | Group
    pub metadata: Metadata,
    pub reference: Option<Reference>,  // [[wikilink]] / [text](path), if any
    pub children: Vec<GoalItem>,
    pub span: Span,
    pub blame_author: Option<String>,  // populated when display.show_git_blame
    pub blame_date: Option<chrono::NaiveDateTime>,
    pub blame_commit: Option<String>,
}

/// One parsed ## GOAL TRACKER section.
pub struct Goal {
    pub title: String,           // first H1, fallback filename stem
    pub source_file: PathBuf,
    pub badge: String,           // super-directory, or "(root)"
    pub items: Vec<GoalItem>,    // top-level checkbox/table items
}

pub enum Status { Planned, Active, Completed }
```

### Derived behaviour

- `Goal::progress() -> f64` — leaf ratio: `done_leaf_count / total_leaf_count`,
  where a leaf is a `GoalItem` with empty `children`, a `Checkbox` state, and
  no `Broken`/`Cycle` reference. Group leaves and dead references do not
  count. **Zero leaves → `0.0`** (no division; status `Planned`).
- `Status::from_progress(p)` — `1.0` → `Completed`, `0.0` → `Planned`,
  otherwise `Active`.
- `GoalItem::is_milestone()` — `!self.children.is_empty()`.
- `Goal::status()` — `Status::from_progress(self.progress())`.
- `Priority` ordering for future sorting: `High > Med > Low > Other > None`
  (untagged). `Other` sorts below `Low` so unknown custom levels never
  outrank the defined triage set.

### Why `Priority::Other(String)`

The spec says unrecognized priority values are "stored as-is". An enum with
an `Other(String)` arm keeps priority in one field while still letting the
triage logic treat `High`/`Med`/`Low` as first-class. `None` (no `!` token
at all) stays distinct from `Other` (an `!` token with an unknown value).

## Configuration (`config.rs`)

Layered loading, last non-absent source wins for scalars; `exclude`/`include`
**merge** (union, de-duplicated) across all layers with the built-in
defaults:

```
built-in defaults  ←  ~/.config/trawl/config.toml  ←  <repo>/.trawl.toml  ←  CLI flags
```

Built-in default excludes: `target/`, `node_modules/`, `.git/`. A project
that sets `exclude = ["docs/"]` still skips those defaults — it does not
re-list them.

```rust
pub struct Config {
    pub scan: ScanConfig,       // keywords, case sensitivity, section names,
                                // include, exclude, max_file_size, scan_hidden
    pub tokens: TokenConfig,    // field → prefix char (owner='@', tag='#', …)
    pub headers: HeaderConfig,  // field → header keywords (task, state, owner…)
    pub display: DisplayConfig, // default_sort, context_lines, thresholds…
}
```

- `max_file_size` is a human string (`"1MB"`) parsed to bytes by a small
  unit parser (KB/MB/GB, case-insensitive).
- `tokens` and `headers` are `HashMap`-backed so users can add custom
  metadata types (e.g. `effort = "%"`) without code changes — this is the
  "extensible by configuration" principle made concrete.

For this revision the CLI contributes `--path` and `--verbose` only; filter
and sort flags arrive with the TUI phase.

## Scanner pipeline (`scanner/`)

The walker applies the filter pipeline exactly as specified in
`requirements.md` → Scan Filtering Semantics:

| Stage | Source | Implementation |
|-------|--------|----------------|
| 1. `.gitignore` | implicit | `ignore::WalkBuilder` standard filters |
| 2. untracked | config | `only_tracked = true` (default): skip files not in `git ls-files` |
| 3. `exclude` | config | `ignore::overrides::Override` (negated globs) or `filter_entry` |
| 4. `include` | config | `ignore::overrides::Override` when non-empty |
| 5. `scan_hidden` | config | `WalkBuilder::hidden(!scan_hidden)` |
| 6. `max_file_size` | config | `filter_entry` checking file length |
| 7. binary | heuristic | null-byte check on read (skip) |

The walker yields candidate paths. `reader.rs` then reads them in parallel
with `rayon`, performs binary detection (skip files containing a `0x00`
byte), and decodes text via `String::from_utf8_lossy` so invalid UTF-8
never aborts a scan. The result is `Vec<FileContents { path, content }>`.

> **Performance note — keyword matching crate.** The requirements list
> `grep-searcher` + `grep-regex` for ripgrep-class SIMD matching. For this
> initial revision we use the simpler `regex` crate to find keyword lines.
> `regex` is more than fast enough at repo scale and keeps the parser
> readable; `grep-searcher` remains the planned optimization for very large
> monorepos. `requirements.md` and `TODO.md` are updated to reflect this.

## Inline task parser (`parser/inline.rs`)

`parse_line(line, ctx) -> Option<InlineTask>`:

1. Find the first keyword occurrence with a word-boundary regex
   `\b(TODO|FIXME|HACK|XXX|BUG)\b` (case sensitivity from config).
   If none, return `None`.
2. Slice the line from the keyword onward (`TODO(auth): … !high`).
3. Read an optional `(scope)`.
4. Consume an optional separator (`:` or one space).
5. The remainder is the raw description; run it through `metadata::extract`
   to strip tokens and yield `(clean_description, Metadata)`.
6. Apply the keyword's default priority unless `metadata.priority` is set
   (`FIXME`/`BUG` → High, `HACK`/`XXX` → Med, `TODO` → none).

The parser degrades gracefully: every shortening of the form — fully
structured, scope-only, separator-only, bare, minimal — yields a valid
`InlineTask`. A keyword with no description at all is still a valid item
with an empty description.

Markdown heading lines (`## TODO: …`) are handled implicitly: the regex
matches `TODO` regardless of leading `#`, so no special heading stripping
is needed beyond slicing from the keyword.

## Goal tracker parser (`parser/goal.rs`)

`parse(content, source_file, ctx) -> Option<Goal>`:

1. **Section detection** — scan headings for the first case-insensitive match
   of a configured section name (defaults: `GOAL TRACKER`, `TODO`) at any level.
   Record that level. The section body extends until the next heading of
   the same or higher level, or EOF. Only the first match is parsed.
2. **Checkbox tree** — for each `- [x]` / `- [ ]` line, compute its indent
   level as `leading_spaces / 2` and attach it to the right parent via an
   indent stack. Everything else inside the section (headings, prose, plain
   bullets, blank lines) is ignored.
3. **Tables** — when a `| … |` row is followed by a `| --- |` separator,
   map header cells to fields by keyword substring (task/state/owner/…).
   Each data row becomes a flat leaf `GoalItem`; its `checked` comes from
   the done-heuristic: `done = NOT (empty OR contains "TODO")`.
4. **Metadata** — tokens embedded in checkbox text or table cells are
   extracted via `metadata::extract`; column-derived values override
   inline tokens for the same field.
5. **Derived fields** — title from the first `# H1` (fallback: filename
   stem), badge from the super-directory, progress and status from the
   leaf ratio.

Tables and checkbox lists may be freely mixed; both contribute items to
the goal's top-level `items` vector.

## Error handling and logging

- `anyhow::Result` is the error type throughout. There is no custom error
  enum: the "resilient parsing" principle means parse failures are skipped,
  not propagated. A parse hiccup is logged at `warn` and the scanner
  continues.
- **The parser never panics on user input.** Index/slice operations are
  bounds-checked; unparseable lines are skipped.
- Logging via `log` + `env_logger`. `--verbose` sets the level to `debug`,
  otherwise `warn`. Logs go to stderr so they never interfere with future
  stdout output.

## Testing strategy

- **TDD for parsers** (per repo parser rules): fixtures under
  `tests/fixtures/{inline,goal}/` are written *before* the parser code.
  Each fixture has a paired assertion.
- **Unit tests** live inline (`#[cfg(test)] mod tests`) for pure logic:
  metadata extraction, progress/status math, badge derivation, indent-stack
  tree building, done-heuristic.
- **Integration tests** under `tests/` drive the public API: build a temp
  tree (`tempfile`), scan it, assert the resulting `ScanResult`.
- **Malformed input is a first-class case.** Every parser has fixtures for
  broken checkbox trees, malformed tables, and unknown tokens; the contract
  is "skip without panicking".
- `cargo fmt --check`, `cargo clippy` (with `-D warnings`), and `cargo test`
  must all pass before any slice is committed.

## Dependencies

Crate choices and why (MVP set; TUI/git crates arrive later):

| Crate | Purpose |
|-------|---------|
| `clap` (derive) | CLI parsing |
| `serde` + `toml` | config deserialization |
| `anyhow` | error handling |
| `log` + `env_logger` | leveled logging |
| `ignore` | `.gitignore`-aware walking + override globs |
| `rayon` | parallel file reading |
| `regex` | keyword line matching (replaces `grep-searcher` for now) |
| `chrono` | `due` date parsing |
| `tempfile` (dev) | scanner integration tests |

New dependencies are flagged here before being added. `tempfile` is the
only one not in `requirements.md` → Key Crates; it is dev-only and
standard for filesystem tests.

## Design decisions and tradeoffs

| Decision | Rationale |
|----------|-----------|
| Unify checkbox + table into `GoalItem` | One progress/render path; a table row is a leaf whose `checked` comes from the heuristic |
| `Priority::Other(String)` | Honors "unrecognized values stored as-is" in one field |
| `regex` over `grep-searcher` (this revision) | Readable MVP; `regex` is fast enough at repo scale; grep crates are a documented later optimization |
| Read each file once, feed both parsers | Simplicity over the dual-read grep/mmap design; files are small |
| `anyhow` only, no error enum | Resilient parsing swallows errors; an enum would be boilerplate with no consumer |
| `from_utf8_lossy` decode | Invalid UTF-8 never aborts a scan; lossy decode matches "parse what you can" |
| `lib.rs` exposes the API | Integration tests drive scanners/parsers without the CLI |
