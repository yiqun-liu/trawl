// Fully structured form: keyword(scope): description @owner #tag !priority ~due
// TODO(auth): handle null user @yiqun #security !high ~2025-12-01

// With scope only (no metadata)
// TODO(auth): handle null user

// With separator only
// TODO: handle null user

// Bare form (space separator, no colon)
// TODO handle null user

// Minimal form (keyword alone)
// TODO

// Different keywords
// FIXME: known bug !high
// HACK: workaround
// NOTE: just a note

// Multiple tags on one line
// TODO: support both #arch #perf

// Block comment context (trailing */ should be stripped from description)
/* TODO: fix this */

// Markdown-style heading is handled elsewhere; this fixture covers C comments.
