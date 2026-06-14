//! Inline tasks view placeholder.
//!
//! The full foldable directory tree arrives in the next slice; for now this
//! renders a summary so Tab navigation between views works end to end.

use ratatui::{
    layout::Rect,
    text::Text,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the inline tasks view (summary only in this slice).
pub(super) fn draw(f: &mut Frame, app: &super::App, area: Rect) {
    let body = format!(
        "Inline tasks: {}\n\n(Foldable directory tree arrives in the next slice.)\n\nPress Tab for Goals.",
        app.inline_tasks.len()
    );
    let para = Paragraph::new(Text::from(body))
        .block(Block::default().borders(Borders::ALL).title("Inline Tasks"));
    f.render_widget(para, area);
}
