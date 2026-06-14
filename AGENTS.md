# AGENTS.md

This file gives repository-specific instructions to coding agents working in
this codebase.

## Start Here

Before making assumptions about this repository, read `README.md`.
If `.AGENTS.md` exists, read it for developer-local paths and
environment assumptions before using nearby reference trees outside this
repository.

Use the repository docs instead of repeating their contents:

- `README.md`: project overview, quick start, and directory structure
- `docs/requirements.md`: feature catalog, design principles, configuration
  reference, and phased implementation plan
- `docs/syntax.md`: formal syntax specification for goal trackers and inline
  tasks — grammar rules, metadata tokens, parsing edge cases
- `docs/guidelines.md`: development guidelines and commit message conventions
- `TODO.md`: implementation progress tracker with phase breakdown
- `docs/README.md`: documentation index

## Design Principles

TRAWL is built on a few core principles. All implementation decisions should
be consistent with these:

- **Scan, don't manage**: TRAWL discovers items from file contents. It does
  not store state in a database — the files *are* the database. Never
  introduce persistent state files, caches, or databases.
- **Pure markdown + inline markers**: no frontmatter, no custom file format.
  Goal trackers are standard markdown sections; inline tasks are standard
  comments. Do not invent new file formats or require users to add metadata
  they didn't already write.
- **Resilient parsing**: the parser must degrade gracefully. Bare `TODO` is
  always valid. Malformed checkbox trees, broken table separators, unknown
  metadata tokens, or unexpected indentation should be handled without
  crashing. Skip what you cannot parse; parse what you can.
- **Two types, one tool**: goals and inline tasks are independently
  discovered and independently displayed. They are peers, not a hierarchy.
  Do not assume a parent-child relationship between them.
- **Extensible by configuration**: metadata token prefixes (`@`, `#`, `!`,
  `~`) and table column mappings are configurable, not hard-coded. When
  adding new metadata types, make them configurable.
- **Binary is the product**: the compiled binary is the end-user interface.
  `--help`, the `?` overlay, and error messages are the user contract and
  must stay accurate and complete; the source repo is contributor
  documentation. When adding or changing user-visible behavior, update the
  binary's help text alongside the code.

## Which Document to Read

Repo docs are written for contributors; end-user guidance lives in the
binary's help system (`--help` and the `?` overlay).

Start with the right document for the task:

- **Understanding what trawl does**: `README.md`, then `docs/requirements.md`
- **Parser or syntax questions**: `docs/syntax.md`
- **Implementation status or what to build next**: `TODO.md`
- **Coding conventions and commit format**: `docs/guidelines.md`

## Build And Verification

Before making a commit, make sure the project builds successfully.

Preferred verification:

```bash
cargo build
cargo clippy --all-targets -- -D warnings
cargo test
```

Format check before committing:

```bash
cargo fmt --check
```

If you cannot run a relevant verification step, say so clearly in your final
report and do not claim it passed.

## Parser Development Rules

When working on the inline task or goal tracker parsers:

- Add test fixtures for any new syntax pattern **before** implementing the
  parser change. Fixtures belong under `tests/fixtures/`.
- Test both well-formed and malformed input. The parser must never panic on
  user input.
- Follow the syntax specification in `docs/syntax.md` exactly. If the spec
  and implementation disagree, ask the user whether to fix the spec or the
  implementation — do not silently choose one.
- When the parser encounters something it cannot handle, skip it and
  continue. Do not abort the scan.
- Prefer simple line-by-line parsing over complex AST construction. Trawl
  reads text files, not structured documents.

## Commit Hygiene

If the user asks for automatic commits, follow these rules:

1. Every commit must leave the project in a buildable state.
2. Keep substantial code or documentation changes separate from procedural
   artifacts.
3. Do not mix one-time implementation plans, progress notes, temporary
   checkpoints, or scratch files into the same commit as real product
   changes unless the user explicitly asks for that.
4. Prefer one commit for the real code/doc change and a separate commit for
   any durable supporting procedural document, if that document truly needs
   to be kept.
5. Remove temporary procedural files once the goal is complete.
6. Do not leave completed implementation plans, temporary progress logs, or
   checkpoint notes behind unless the user explicitly wants them preserved.

Examples of procedural artifacts that should usually be cleaned up before
the final commit:

- one-time implementation plans
- temporary migration checklists
- progress logs
- scratch notes
- ad hoc validation files created only for the task

For commit message format, see `docs/guidelines.md`.

## Editing Guidance

Prefer minimal, local changes that fit the existing style.

When reorganizing files:

- update affected documentation links
- keep paths consistent with the audience split
- do not leave stale references behind

When refactoring:

- preserve user-visible behavior unless the task explicitly changes it
- keep naming aligned with the actual behavior, especially for config keys,
  CLI flags, and metadata token prefixes

When adding dependencies:

- check `docs/requirements.md` → Key Crates for the intended crate list
- prefer crates already in the project over introducing new ones
- flag any new dependency to the user before adding it
