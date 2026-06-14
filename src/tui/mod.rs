//! Terminal user interface.
//!
//! The TUI takes a [`ScanResult`] and renders an interactive two-view
//! dashboard. Terminal setup/restore is centralized so a panic still restores
//! the user's terminal. Pure display logic (row flattening) lives in
//! `goals.rs` so it can be unit-tested without a terminal.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Clear, Paragraph},
    Frame, Terminal,
};

use crate::model::{Goal, GoalItem, InlineTask, Status};
use crate::ScanResult;

mod filter;
mod goals;
mod inline_view;

use filter::Filter;
use goals::{flatten_goals, GoalRow, GoalRowKind};
use inline_view::{auto_expand_keys, build_tree, flatten_inline, InlineRow, TreeNode};

type Tui = Terminal<CrosstermBackend<Stdout>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Goals,
    Inline,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    FilterInput,
}

/// Run the TUI over a scan result. Restores the terminal on exit or panic.
pub fn run(result: ScanResult, root: PathBuf) -> Result<()> {
    let mut app = App::new(result.goals, result.inline_tasks, root);

    let mut terminal = setup_terminal()?;
    // If the main loop errors, still restore the terminal before returning.
    let outcome = run_loop(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    outcome
}

fn setup_terminal() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn restore_terminal(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn run_loop(terminal: &mut Tui, app: &mut App) -> Result<()> {
    // Restore the terminal even if a panic occurs mid-loop.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        default_hook(info);
    }));

    while !app.quit {
        terminal.draw(|f| draw(f, app))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let event::Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        handle_key(app, key);
    }
    Ok(())
}

fn handle_key(app: &mut App, key: event::KeyEvent) {
    // Ctrl+C always quits, even mid-filter.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }

    // The help overlay is modal: only `?`/Esc close it, `q` quits.
    if app.show_help {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => app.show_help = false,
            KeyCode::Char('q') | KeyCode::Char('Q') => app.quit = true,
            _ => {}
        }
        return;
    }

    if app.mode == Mode::FilterInput {
        match key.code {
            KeyCode::Enter => app.apply_filter(),
            KeyCode::Esc => app.cancel_filter(),
            KeyCode::Backspace => {
                app.filter_input.pop();
            }
            KeyCode::Char(c) => app.filter_input.push(c),
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => app.quit = true,
        KeyCode::Tab => app.toggle_view(),
        KeyCode::Char('j') | KeyCode::Down => app.move_cursor(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_cursor(-1),
        KeyCode::Char('l') => app.expand_selected(),
        KeyCode::Enter => app.toggle_selected(),
        KeyCode::Char('h') | KeyCode::Backspace => app.collapse_selected(),
        KeyCode::Char('C') => app.collapse_completed(),
        KeyCode::Char('Z') => app.collapse_all(),
        KeyCode::Char('f') => app.begin_filter(),
        KeyCode::Esc => app.clear_filter(),
        KeyCode::Char(' ') => app.toggle_checkbox(),
        KeyCode::Char('?') => app.show_help = true,
        _ => {}
    }
}

fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(f.area());
    let (main, footer) = (chunks[0], chunks[1]);

    match app.view {
        View::Goals => goals::draw(f, app, main),
        View::Inline => inline_view::draw(f, app, main),
    }

    let footer_text = if app.mode == Mode::FilterInput {
        format!("filter> {}", app.filter_input)
    } else {
        match app.view {
            View::Goals => {
                "Enter: toggle  l: expand  h: collapse  Space: toggle box  C: done  Z: all  j/k  Tab  q".to_string()
            }
            View::Inline if app.filter.is_some() => {
                format!("filter: \"{}\"  f: edit  Esc: clear  Z: collapse all  Tab: Goals  q: quit", app.filter_query)
            }
            View::Inline => {
                "f: filter  Enter: toggle  l/h: expand/collapse  Z: collapse all  j/k: move  Tab: Goals  q: quit".to_string()
            }
        }
    };
    let footer_widget = Paragraph::new(Line::from(footer_text))
        .style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_widget(footer_widget, footer);

    if app.show_help {
        draw_help(f, app.view);
    }
}

/// Render the modal help overlay on top of the current view.
fn draw_help(f: &mut Frame, view: View) {
    let area = centered_rect(64, 80, f.area());
    f.render_widget(Clear, area);
    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .title("Keybindings  (press ? or Esc to close)");
    f.render_widget(
        ratatui::widgets::Paragraph::new(help_text(view)).block(block),
        area,
    );
}

/// Per-view keybinding text for the help overlay.
fn help_text(view: View) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from("Navigation"),
        Line::from("  j / k        move down / up"),
        Line::from("  l / h        expand / collapse"),
        Line::from("  Enter        toggle  (on a leaf, toggles its parent)"),
        Line::from("  Space        toggle checkbox  (goals view; writes back)"),
        Line::from("  Tab          switch Goals <-> Inline Tasks"),
        Line::from(""),
        Line::from("Goals & Milestones"),
        Line::from("  C            collapse fully-complete nodes"),
        Line::from("  Z            collapse all (current view)"),
    ];
    if view == View::Inline {
        lines.push(Line::from(""));
        lines.push(Line::from("Inline tasks"));
        lines.push(Line::from("  f            filter prompt"));
        lines.push(Line::from(
            "               (kw: pri: tag: owner: path: <text>)",
        ));
        lines.push(Line::from("  Esc          clear filter"));
    }
    lines.push(Line::from(""));
    lines.push(Line::from("  ?            toggle this help"));
    lines.push(Line::from("  q / Ctrl+C   quit"));
    lines
}

/// A rectangle centered in `area`, `pct_x`% wide and `pct_y`% tall.
fn centered_rect(pct_x: u16, pct_y: u16, area: Rect) -> Rect {
    let [_top, mid, _bottom] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - pct_y) / 2),
            Constraint::Percentage(pct_y),
            Constraint::Percentage((100 - pct_y) / 2),
        ])
        .areas(area);
    let [_left, center, _right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - pct_x) / 2),
            Constraint::Percentage(pct_x),
            Constraint::Percentage((100 - pct_x) / 2),
        ])
        .areas(mid);
    center
}

/// Application state for the TUI.
struct App {
    goals: Vec<Goal>,
    inline_tasks: Vec<InlineTask>,
    inline_displayed: Vec<InlineTask>,
    root: PathBuf,
    view: View,
    mode: Mode,
    filter: Option<Filter>,
    filter_query: String,
    filter_input: String,
    goal_rows: Vec<GoalRow>,
    goal_selected: usize,
    goal_expanded: HashSet<String>,
    inline_root: TreeNode,
    inline_rows: Vec<InlineRow>,
    inline_selected: usize,
    expanded_inline: HashSet<String>,
    quit: bool,
    show_help: bool,
}

impl App {
    fn new(goals: Vec<Goal>, inline_tasks: Vec<InlineTask>, root: PathBuf) -> Self {
        let goal_expanded = HashSet::new();
        let goal_rows = flatten_goals(&goals, &goal_expanded);

        let inline_displayed = inline_tasks.clone();
        let inline_root = build_tree(&inline_displayed);
        let expanded_inline = auto_expand_keys(&inline_root, &inline_displayed);
        let inline_rows = flatten_inline(&inline_root, &inline_displayed, &expanded_inline);

        Self {
            goals,
            inline_tasks,
            inline_displayed,
            root,
            view: View::Goals,
            mode: Mode::Normal,
            filter: None,
            filter_query: String::new(),
            filter_input: String::new(),
            goal_rows,
            goal_selected: 0,
            goal_expanded,
            inline_root,
            inline_rows,
            inline_selected: 0,
            expanded_inline,
            quit: false,
            show_help: false,
        }
    }

    /// `f`: begin (or edit) the filter query.
    fn begin_filter(&mut self) {
        self.mode = Mode::FilterInput;
        self.filter_input = self.filter_query.clone();
    }

    /// Enter in filter mode: parse and apply the typed query.
    fn apply_filter(&mut self) {
        let parsed = Filter::parse(&self.filter_input);
        if parsed.is_empty() {
            self.filter = None;
            self.filter_query.clear();
        } else {
            self.filter_query = self.filter_input.clone();
            self.filter = Some(parsed);
        }
        self.mode = Mode::Normal;
        self.rebuild_inline();
        self.inline_selected = 0;
    }

    /// Esc in filter mode: discard the typed text, keep the prior filter.
    fn cancel_filter(&mut self) {
        self.mode = Mode::Normal;
    }

    /// Esc in normal mode: clear the active filter entirely.
    fn clear_filter(&mut self) {
        if self.filter.take().is_some() {
            self.filter_query.clear();
            self.rebuild_inline();
            self.inline_selected = 0;
        }
    }

    /// Recompute the displayed inline tasks from the current filter, then
    /// rebuild the tree/rows. Called when the filter changes.
    fn rebuild_inline(&mut self) {
        self.inline_displayed = match &self.filter {
            None => self.inline_tasks.clone(),
            Some(f) => self
                .inline_tasks
                .iter()
                .filter(|t| f.matches(t))
                .cloned()
                .collect(),
        };
        self.rebuild_inline_rows();
        if !self.inline_rows.is_empty() {
            self.inline_selected = self.inline_selected.min(self.inline_rows.len() - 1);
        } else {
            self.inline_selected = 0;
        }
    }

    /// Rebuild the inline tree/rows from the currently displayed tasks. Does
    /// not refilter; called after expand/collapse.
    fn rebuild_inline_rows(&mut self) {
        self.inline_root = build_tree(&self.inline_displayed);
        self.inline_rows = flatten_inline(
            &self.inline_root,
            &self.inline_displayed,
            &self.expanded_inline,
        );
    }

    fn toggle_view(&mut self) {
        self.view = match self.view {
            View::Goals => View::Inline,
            View::Inline => View::Goals,
        };
    }

    fn move_cursor(&mut self, delta: i32) {
        match self.view {
            View::Goals => {
                let len = self.goal_rows.len();
                if len != 0 {
                    let next = (self.goal_selected as i32 + delta).clamp(0, (len - 1) as i32);
                    self.goal_selected = next as usize;
                }
            }
            View::Inline => {
                let len = self.inline_rows.len();
                if len != 0 {
                    let next = (self.inline_selected as i32 + delta).clamp(0, (len - 1) as i32);
                    self.inline_selected = next as usize;
                }
            }
        }
    }

    fn expand_selected(&mut self) {
        let Some(key) = self.current_key() else {
            return;
        };
        let newly = match self.view {
            View::Goals => self.goal_expanded.insert(key.clone()),
            View::Inline => self.expanded_inline.insert(key.clone()),
        };
        if newly {
            self.rebuild_active();
        }
    }

    fn collapse_selected(&mut self) {
        let Some(key) = self.current_key() else {
            return;
        };
        let was_open = match self.view {
            View::Goals => self.goal_expanded.remove(&key),
            View::Inline => self.expanded_inline.remove(&key),
        };
        if was_open {
            self.rebuild_active();
            self.seek_cursor(&key);
        }
    }

    /// `Space` (goals view): flip the selected item's checkbox `[x]`/`[ ]` in
    /// the source file and in memory. Goal headers and non-checkbox lines are
    /// left untouched.
    fn toggle_checkbox(&mut self) {
        if self.view != View::Goals {
            return;
        }
        let Some(item_key) = self.selected_goal_item_key() else {
            return; // goal header, or nothing selected
        };
        let Some((gi, path)) = parse_item_key(&item_key) else {
            return;
        };
        let Some((rel, line)) = goal_item_span(&self.goals, gi, &path) else {
            return;
        };

        let abs = self.root.join(&rel);
        let Ok(content) = fs::read_to_string(&abs) else {
            log::warn!("toggle: cannot read {}", abs.display());
            return;
        };
        let mut lines: Vec<String> = content.split('\n').map(String::from).collect();
        let idx = line.saturating_sub(1);
        if idx >= lines.len() {
            log::warn!("toggle: line {line} out of range in {}", abs.display());
            return;
        }
        let Some(new_line) = goals::flip_checkbox(&lines[idx]) else {
            log::debug!("toggle: not a checkbox line at {}:{}", abs.display(), line);
            return;
        };
        lines[idx] = new_line;
        if fs::write(&abs, lines.join("\n")).is_err() {
            log::warn!("toggle: cannot write {}", abs.display());
            return;
        }

        if let Some(item) = goal_item_mut(&mut self.goals, gi, &path) {
            item.checked = !item.checked;
        }
        self.goal_rows = flatten_goals(&self.goals, &self.goal_expanded);
    }

    /// The key of the selected row in the active view. For a leaf, this is
    /// its parent's key, so expand/collapse/toggle act on the parent.
    fn current_key(&self) -> Option<String> {
        match self.view {
            View::Goals => self.selected_goal_key(),
            View::Inline => self.selected_inline_key(),
        }
    }

    /// Rebuild the active view's rows from its expand set.
    fn rebuild_active(&mut self) {
        match self.view {
            View::Goals => {
                self.goal_rows = flatten_goals(&self.goals, &self.goal_expanded);
            }
            View::Inline => self.rebuild_inline_rows(),
        }
    }

    /// Move the active-view cursor onto the row whose node key is `key`.
    fn seek_cursor(&mut self, key: &str) {
        let pos = match self.view {
            View::Goals => self
                .goal_rows
                .iter()
                .position(|r| goal_row_node_key(r).is_some_and(|k| k == key)),
            View::Inline => self
                .inline_rows
                .iter()
                .position(|r| inline_row_node_key(r).is_some_and(|k| k == key)),
        };
        if let Some(i) = pos {
            match self.view {
                View::Goals => self.goal_selected = i,
                View::Inline => self.inline_selected = i,
            }
        }
    }

    /// `C`: collapse every node whose subtree is fully complete -- a goal
    /// with status Completed, or a milestone that is itself checked and whose
    /// leaves are all checked. Hierarchical: intermediate nodes fold too.
    fn collapse_completed(&mut self) {
        let mut to_remove: Vec<String> = Vec::new();
        for (gi, goal) in self.goals.iter().enumerate() {
            let gkey = format!("g{gi}");
            if self.goal_expanded.contains(&gkey) && goal.status() == Status::Completed {
                to_remove.push(gkey);
            }
            for (ci, item) in goal.items.iter().enumerate() {
                collect_done_keys(
                    item,
                    &format!("g{gi}/{ci}"),
                    &self.goal_expanded,
                    &mut to_remove,
                );
            }
        }
        if to_remove.is_empty() {
            return;
        }
        for key in to_remove {
            self.goal_expanded.remove(&key);
        }
        self.goal_rows = flatten_goals(&self.goals, &self.goal_expanded);
    }

    /// `Z`: collapse everything in the active view.
    fn collapse_all(&mut self) {
        let changed = match self.view {
            View::Goals => !self.goal_expanded.is_empty(),
            View::Inline => !self.expanded_inline.is_empty(),
        };
        if !changed {
            return;
        }
        match self.view {
            View::Goals => self.goal_expanded.clear(),
            View::Inline => self.expanded_inline.clear(),
        };
        self.rebuild_active();
        self.clamp_active_cursor();
    }

    /// Keep the active cursor within the active view's row count.
    fn clamp_active_cursor(&mut self) {
        match self.view {
            View::Goals => {
                let len = self.goal_rows.len();
                self.goal_selected = if len == 0 {
                    0
                } else {
                    self.goal_selected.min(len - 1)
                };
            }
            View::Inline => {
                let len = self.inline_rows.len();
                self.inline_selected = if len == 0 {
                    0
                } else {
                    self.inline_selected.min(len - 1)
                };
            }
        }
    }

    /// Enter: toggle the selected node (fold ↔ unfold). On a leaf, toggles
    /// the parent and moves the cursor onto it.
    fn toggle_selected(&mut self) {
        let Some(key) = self.current_key() else {
            return;
        };
        let now_open = match self.view {
            View::Goals => {
                if self.goal_expanded.contains(&key) {
                    self.goal_expanded.remove(&key);
                    false
                } else {
                    self.goal_expanded.insert(key.clone());
                    true
                }
            }
            View::Inline => {
                if self.expanded_inline.contains(&key) {
                    self.expanded_inline.remove(&key);
                    false
                } else {
                    self.expanded_inline.insert(key.clone());
                    true
                }
            }
        };
        self.rebuild_active();
        if !now_open {
            self.seek_cursor(&key);
        }
    }

    /// The key of the selected goals-view row. For a leaf task, returns its
    /// parent milestone/goal key so fold actions target the parent.
    fn selected_goal_key(&self) -> Option<String> {
        self.goal_rows
            .get(self.goal_selected)
            .map(|row| match &row.kind {
                GoalRowKind::Header { key, .. } | GoalRowKind::Milestone { key } => key.clone(),
                GoalRowKind::Task { parent_key, .. } => parent_key.clone(),
            })
    }

    /// The own key of the selected goal *item* (milestone or leaf), or None
    /// for a goal header. Used by `Space` to locate the `GoalItem` to toggle.
    fn selected_goal_item_key(&self) -> Option<String> {
        self.goal_rows
            .get(self.goal_selected)
            .and_then(|row| match &row.kind {
                GoalRowKind::Header { .. } => None,
                GoalRowKind::Milestone { key } | GoalRowKind::Task { key, .. } => Some(key.clone()),
            })
    }

    /// The key of the selected inline-view row. For a leaf task, returns its
    /// file's key so fold actions target the file.
    fn selected_inline_key(&self) -> Option<String> {
        self.inline_rows
            .get(self.inline_selected)
            .map(|row| match &row.kind {
                inline_view::InlineRowKind::Dir(k) | inline_view::InlineRowKind::File(k) => {
                    k.clone()
                }
                inline_view::InlineRowKind::Task { parent_key } => parent_key.clone(),
            })
    }
}

/// The foldable key of a goals-view row, or `None` for leaves.
fn goal_row_node_key(row: &GoalRow) -> Option<&str> {
    match &row.kind {
        GoalRowKind::Header { key, .. } | GoalRowKind::Milestone { key } => Some(key),
        GoalRowKind::Task { .. } => None,
    }
}

/// Parse an item key `g{gi}/{c0}/{c1}/...` into the goal index and the
/// child-index path to the item.
fn parse_item_key(key: &str) -> Option<(usize, Vec<usize>)> {
    let mut parts = key.split('/');
    let gi = parts.next()?.strip_prefix('g')?.parse::<usize>().ok()?;
    let path = parts.filter_map(|p| p.parse::<usize>().ok()).collect();
    Some((gi, path))
}

/// Borrow the goal item at `(gi, path)` immutably.
fn goal_item_ref<'a>(goals: &'a [Goal], gi: usize, path: &[usize]) -> Option<&'a GoalItem> {
    let goal = goals.get(gi)?;
    if path.is_empty() {
        return None;
    }
    let mut cur = goal.items.get(path[0])?;
    for &idx in &path[1..] {
        cur = cur.children.get(idx)?;
    }
    Some(cur)
}

/// Borrow the goal item at `(gi, path)` mutably.
fn goal_item_mut<'a>(goals: &'a mut [Goal], gi: usize, path: &[usize]) -> Option<&'a mut GoalItem> {
    let goal = goals.get_mut(gi)?;
    if path.is_empty() {
        return None;
    }
    let mut cur = goal.items.get_mut(path[0])?;
    for &idx in &path[1..] {
        cur = cur.children.get_mut(idx)?;
    }
    Some(cur)
}

/// The `(relative_path, 1-based line)` of the goal item at `(gi, path)`.
fn goal_item_span(goals: &[Goal], gi: usize, path: &[usize]) -> Option<(PathBuf, usize)> {
    let item = goal_item_ref(goals, gi, path)?;
    Some((item.span.path.clone(), item.span.line))
}

/// The foldable key of an inline-view row, or `None` for leaves.
fn inline_row_node_key(row: &InlineRow) -> Option<&str> {
    match &row.kind {
        inline_view::InlineRowKind::Dir(k) | inline_view::InlineRowKind::File(k) => Some(k),
        inline_view::InlineRowKind::Task { .. } => None,
    }
}

/// Count `(total_leaf, done_leaf)` beneath an item.
fn item_leaf_counts(item: &GoalItem) -> (usize, usize) {
    let mut total = 0usize;
    let mut done = 0usize;
    count_item_leaves(item, &mut total, &mut done);
    (total, done)
}

fn count_item_leaves(item: &GoalItem, total: &mut usize, done: &mut usize) {
    if item.children.is_empty() {
        *total += 1;
        if item.checked {
            *done += 1;
        }
    } else {
        for child in &item.children {
            count_item_leaves(child, total, done);
        }
    }
}

/// A milestone is "done" when it is itself checked and all its leaves are
/// checked.
fn subtree_done(item: &GoalItem) -> bool {
    if !item.checked {
        return false;
    }
    let (total, done) = item_leaf_counts(item);
    total > 0 && done == total
}

/// Collect keys of expanded, fully-done milestone nodes (recursive).
fn collect_done_keys(
    item: &GoalItem,
    key: &str,
    expanded: &HashSet<String>,
    out: &mut Vec<String>,
) {
    if item.children.is_empty() {
        return;
    }
    if expanded.contains(key) && subtree_done(item) {
        out.push(key.to_string());
    }
    for (ci, child) in item.children.iter().enumerate() {
        collect_done_keys(child, &format!("{key}/{ci}"), expanded, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Metadata, Span};
    use std::path::PathBuf;

    fn leaf(text: &str, checked: bool) -> GoalItem {
        GoalItem {
            text: text.into(),
            checked,
            metadata: Metadata::default(),
            children: Vec::new(),
            span: Span {
                path: PathBuf::from("x.md"),
                line: 1,
            },
        }
    }

    fn milestone(text: &str, checked: bool, children: Vec<GoalItem>) -> GoalItem {
        GoalItem {
            text: text.into(),
            checked,
            metadata: Metadata::default(),
            children,
            span: Span {
                path: PathBuf::from("x.md"),
                line: 1,
            },
        }
    }

    #[test]
    fn subtree_done_requires_self_and_all_leaves_checked() {
        // self checked, all leaves checked -> done
        let m = milestone("m", true, vec![leaf("a", true), leaf("b", true)]);
        assert!(subtree_done(&m));
        // self unchecked -> not done even if leaves are
        let m = milestone("m", false, vec![leaf("a", true), leaf("b", true)]);
        assert!(!subtree_done(&m));
        // a leaf unchecked -> not done
        let m = milestone("m", true, vec![leaf("a", true), leaf("b", false)]);
        assert!(!subtree_done(&m));
    }

    #[test]
    fn subtree_done_handles_nested_milestones() {
        // fully done nested
        let inner = milestone("inner", true, vec![leaf("a", true)]);
        let outer = milestone("outer", true, vec![inner]);
        assert!(subtree_done(&outer));
        // a deep leaf unchecked -> not done (intermediate milestone checkboxes
        // are user-controlled and do not affect completion).
        let inner = milestone("inner", true, vec![leaf("a", false)]);
        let outer = milestone("outer", true, vec![inner]);
        assert!(!subtree_done(&outer));
    }
}
