//! Terminal user interface.
//!
//! The TUI takes a [`ScanResult`] and renders an interactive two-view
//! dashboard. Terminal setup/restore is centralized so a panic still restores
//! the user's terminal. Pure display logic (row flattening) lives in
//! `goals.rs` so it can be unit-tested without a terminal.

use std::collections::HashSet;
use std::io::{self, Stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::Line,
    widgets::Paragraph,
    Frame, Terminal,
};

use crate::model::{Goal, InlineTask, Status};
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
pub fn run(result: ScanResult) -> Result<()> {
    let mut app = App::new(result.goals, result.inline_tasks);

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
        KeyCode::Char('f') => app.begin_filter(),
        KeyCode::Esc => app.clear_filter(),
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
                "Enter: toggle  l: expand  h: collapse  C: collapse done  j/k: move  Tab: Inline Tasks  q: quit".to_string()
            }
            View::Inline if app.filter.is_some() => {
                format!("filter: \"{}\"  f: edit  Esc: clear  Tab: Goals  q: quit", app.filter_query)
            }
            View::Inline => {
                "f: filter  Enter: toggle  l/h: expand/collapse  j/k: move  Tab: Goals  q: quit".to_string()
            }
        }
    };
    let footer_widget = Paragraph::new(Line::from(footer_text))
        .style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_widget(footer_widget, footer);
}

/// Application state for the TUI.
struct App {
    goals: Vec<Goal>,
    inline_tasks: Vec<InlineTask>,
    inline_displayed: Vec<InlineTask>,
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
}

impl App {
    fn new(goals: Vec<Goal>, inline_tasks: Vec<InlineTask>) -> Self {
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

    /// `C`: collapse every completed (100%) goal in one keystroke.
    fn collapse_completed(&mut self) {
        let mut changed = false;
        for (gi, goal) in self.goals.iter().enumerate() {
            if goal.status() == Status::Completed && self.goal_expanded.remove(&format!("g{gi}")) {
                changed = true;
            }
        }
        if changed {
            self.goal_rows = flatten_goals(&self.goals, &self.goal_expanded);
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
                GoalRowKind::Task { parent_key } => parent_key.clone(),
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

/// The foldable key of an inline-view row, or `None` for leaves.
fn inline_row_node_key(row: &InlineRow) -> Option<&str> {
    match &row.kind {
        inline_view::InlineRowKind::Dir(k) | inline_view::InlineRowKind::File(k) => Some(k),
        inline_view::InlineRowKind::Task { .. } => None,
    }
}
