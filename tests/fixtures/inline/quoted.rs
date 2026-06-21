// Fixture: quoted-keyword skipping. The keyword occurrences below are inside
// string literals or code spans and must be skipped; the two bare comment
// lines must be reported.
fn example() {
    let _ = "TODO".into();
    let _ = "FIXME".to_string();
    // see `HACK` above for context
    // TODO: real task one
    // BUG: real bug
}
