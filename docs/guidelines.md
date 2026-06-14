# TRAWL — Development Guidelines

Coding and workflow guidelines for contributors.

## Git Commit Messages

Follow the [Conventional Commits](https://www.conventionalcommits.org/)
specification.

### Format

```
type(scope): description

body (optional)

footer (optional)
```

### Types

| Type | Use for |
|------|---------|
| `feat` | New feature or capability |
| `fix` | Bug fix |
| `docs` | Documentation changes |
| `refactor` | Code restructuring without behavior change |
| `test` | Adding or updating tests |
| `chore` | Tooling, dependencies, config |
| `perf` | Performance improvement |
| `style` | Formatting, no logic change |

### Scope

The scope is optional and maps to the module affected:

`scanner`, `parser`, `tui`, `config`, `cli`

Omit the scope when the change spans multiple modules or is non-code.

### Rules

- Subject line in **imperative mood** ("add" not "added" or "adds")
- Subject line **lowercase**, no period
- Subject line **max 72 characters**
- Body wrapped at 72 characters
- Body explains **what** changed and **why**, not how
- **One logical change per commit**
- Reference issues in the footer: `Closes #N`

### Examples

```
feat(scanner): add gitignore-aware directory walker
```

```
fix(parser): handle unchecked checkbox state in goal tracker

A checkbox with `[ ]` (space, no x) was incorrectly parsed as
checked because the state check only looked for 'x', not the
absence of it.
```

```
docs: add development guidelines
```

```
refactor(tui): extract goal view rendering into widget
```

```
test(parser): add fixtures for table format parsing
```

---

## Core Philosophy

TODO

## Module Boundaries

TODO

## Function Design

TODO

## Error Handling

TODO

## Testing

Treat tests as part of the feature, not an afterthought.

- **Parser changes are TDD.** Add a fixture under `tests/fixtures/inline/`
  or `tests/fixtures/goal/` *before* the parser code. The fixture encodes
  the expected behavior; the test asserts it.
- **Unit tests are inline.** Pure logic lives in `#[cfg(test)] mod tests`
  within the module it tests (metadata extraction, progress math, badge
  derivation, row flattening). Keep terminal/IO out of these so they run
  fast and headless.
- **Integration tests drive the public API.** Tests under `tests/` build a
  temporary tree (`tempfile`), scan it, and assert on the `ScanResult`.
  They never touch a real terminal.
- **Malformed input is a first-class case.** Every parser has a "does not
  panic on malformed input" contract: unparseable lines, broken tables, and
  unknown tokens are skipped. The scanner continues; the user never sees a
  crash from a bad file.
- **The commit gate is four green checks:**
  `cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test`,
  and `cargo fmt --check`. All must pass before a commit.

See `docs/design/architecture.md` → Testing strategy for the rationale.

## Comments

TODO

---

> This document is still evolving. Sections marked TODO will be
> filled in as the codebase matures and patterns emerge.
