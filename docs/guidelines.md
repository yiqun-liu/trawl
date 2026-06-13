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

TODO

## Comments

TODO

---

> This document is still evolving. Sections marked TODO will be
> filled in as the codebase matures and patterns emerge.
