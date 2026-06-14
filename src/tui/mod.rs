//! Terminal user interface.
//!
//! The TUI takes a [`ScanResult`] and renders an interactive two-view
//! dashboard. Terminal setup/restore is centralized so a panic still restores
//! the user's terminal. Pure display logic (row flattening) lives in
//! `goals.rs` so it can be unit-tested without a terminal.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
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
    CellEdit,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SortMode {
    Path,
    Priority,
    Keyword,
}

impl SortMode {
    /// Cycle to the next active mode (excludes `Age` until Phase 3).
    fn next(self) -> Self {
        match self {
            SortMode::Path => SortMode::Priority,
            SortMode::Priority => SortMode::Keyword,
            SortMode::Keyword => SortMode::Path,
        }
    }
}

/// Run the TUI over a scan result. Restores the terminal on exit or panic.
pub fn run(result: ScanResult, root: PathBuf, headers: HashMap<String, Vec<String>>) -> Result<()> {
    let mut app = App::new(result.goals, result.inline_tasks, root, headers);

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

        if let Some((rel, line)) = app.pending_edit.take() {
            let abs = app.root.join(&rel);
            disable_raw_mode()?;
            execute!(io::stdout(), LeaveAlternateScreen)?;

            let _ = editor_command(&abs, line).status();

            *terminal = setup_terminal()?;
        }
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

    if app.show_stats {
        match key.code {
            KeyCode::Char('S') | KeyCode::Esc => app.show_stats = false,
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

    if app.mode == Mode::CellEdit {
        match key.code {
            KeyCode::Enter => app.apply_cell_edit(),
            KeyCode::Esc => {
                app.cell_edit = None;
                app.cell_input.clear();
                app.mode = Mode::Normal;
            }
            KeyCode::Backspace => {
                app.cell_input.pop();
            }
            KeyCode::Char(c) => app.cell_input.push(c),
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
        KeyCode::Char('X') => app.expand_all(),
        KeyCode::Char('S') => app.show_stats = true,
        KeyCode::Char('f') => app.begin_filter(),
        KeyCode::Char('s') => app.cycle_sort(),
        KeyCode::Char('g') => app.toggle_blame(),
        KeyCode::Esc => app.clear_filter(),
        KeyCode::Char('e') => app.edit_selected(),
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

    let footer_text = if app.mode == Mode::CellEdit {
        format!("edit state column > {}", app.cell_input)
    } else if app.mode == Mode::FilterInput {
        format!("filter> {}", app.filter_input)
    } else {
        match app.view {
            View::Goals => {
                "Enter: toggle  l: expand  h: collapse  Space: toggle box  e: edit  S: stats  C: done  Z: all  X: expand all  j/k  Tab  q".to_string()
            }
            View::Inline if app.filter.is_some() => {
                format!("filter: \"{}\"  f: edit  Esc: clear  e: edit  s: sort  S: stats  Z: all  X: expand  Tab: Goals  q: quit", app.filter_query)
            }
            View::Inline => {
                "f: filter  s: sort  g: blame  Enter: toggle  l/h: fold  e: edit  S: stats  Z: all  X: expand  j/k  Tab: Goals  q: quit".to_string()
            }
        }
    };
    let footer_widget = Paragraph::new(Line::from(footer_text))
        .style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_widget(footer_widget, footer);

    if app.show_help {
        draw_help(f, app.view);
    }
    if app.mode == Mode::CellEdit {
        draw_cell_edit(f, &app.cell_input);
    }
    if app.show_stats {
        draw_stats(f, app);
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

/// Render the cell-edit popup, pre-filled with the current state-cell value.
fn draw_cell_edit(f: &mut Frame, input: &str) {
    let area = centered_rect(50, 16, f.area());
    f.render_widget(Clear, area);
    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .title("Edit state  (Enter: save  Esc: cancel)");
    f.render_widget(
        ratatui::widgets::Paragraph::new(Line::from(format!("> {}█", input))).block(block),
        area,
    );
}

/// Render the stats dashboard popup.
fn draw_stats(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 70, f.area());
    f.render_widget(Clear, area);
    let block = ratatui::widgets::Block::default()
        .borders(ratatui::widgets::Borders::ALL)
        .title("Stats Dashboard  (S or Esc to close)");
    f.render_widget(
        ratatui::widgets::Paragraph::new(app.compute_stats()).block(block),
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
        Line::from("  g            toggle git blame (inline view)"),
        Line::from("  e            edit file at cursor"),
        Line::from("  Tab          switch Goals <-> Inline Tasks"),
        Line::from(""),
        Line::from("Goals & Milestones"),
        Line::from("  C            collapse fully-complete nodes"),
        Line::from("  Z            collapse all (current view)"),
        Line::from("  X            expand all (current view)"),
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
    headers: HashMap<String, Vec<String>>,
    sort_mode: SortMode,
    view: View,
    mode: Mode,
    filter: Option<Filter>,
    filter_query: String,
    filter_input: String,
    cell_input: String,
    cell_edit: Option<CellTarget>,
    goal_rows: Vec<GoalRow>,
    goal_selected: usize,
    goal_expanded: HashSet<String>,
    inline_root: TreeNode,
    inline_rows: Vec<InlineRow>,
    inline_selected: usize,
    expanded_inline: HashSet<String>,
    quit: bool,
    show_help: bool,
    show_stats: bool,
    show_blame: bool,
    pending_edit: Option<(PathBuf, usize)>,
}

/// In-flight table-cell edit: which goal item, which source line, which column.
struct CellTarget {
    gi: usize,
    path: Vec<usize>,
    line: usize,
    state_col: usize,
}

impl App {
    fn new(
        goals: Vec<Goal>,
        inline_tasks: Vec<InlineTask>,
        root: PathBuf,
        headers: HashMap<String, Vec<String>>,
    ) -> Self {
        let goal_expanded = HashSet::new();
        let goal_rows = flatten_goals(&goals, &goal_expanded, false);

        let inline_displayed = inline_tasks.clone();
        let inline_root = build_tree(&inline_displayed);
        let expanded_inline = auto_expand_keys(&inline_root, &inline_displayed);
        let inline_rows = flatten_inline(&inline_root, &inline_displayed, &expanded_inline, false);

        Self {
            goals,
            inline_tasks,
            inline_displayed,
            root,
            headers,
            sort_mode: SortMode::Path,
            view: View::Goals,
            mode: Mode::Normal,
            filter: None,
            filter_query: String::new(),
            filter_input: String::new(),
            cell_input: String::new(),
            cell_edit: None,
            goal_rows,
            goal_selected: 0,
            goal_expanded,
            inline_root,
            inline_rows,
            inline_selected: 0,
            expanded_inline,
            quit: false,
            show_help: false,
            show_stats: false,
            show_blame: false,
            pending_edit: None,
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

    /// Compute stats lines for the `S` dashboard.
    fn compute_stats(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        let mut prio = [0usize; 5];
        for t in &self.inline_tasks {
            match &t.metadata.priority {
                Some(crate::model::Priority::High) => prio[0] += 1,
                Some(crate::model::Priority::Med) => prio[1] += 1,
                Some(crate::model::Priority::Low) => prio[2] += 1,
                Some(crate::model::Priority::Other(_)) => prio[3] += 1,
                None => prio[4] += 1,
            }
        }
        lines.push(Line::from(""));
        lines.push(Line::from("Priority"));
        lines.push(Line::from(format!(
            "  high:{}  med:{}  low:{}  other:{}  untagged:{}",
            prio[0], prio[1], prio[2], prio[3], prio[4]
        )));

        let mut kw: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for t in &self.inline_tasks {
            *kw.entry(t.keyword.to_uppercase()).or_default() += 1;
        }
        lines.push(Line::from(""));
        lines.push(Line::from("Keyword"));
        let kw_list: Vec<String> = kw.iter().map(|(k, c)| format!("  {k}:{c}")).collect();
        lines.push(Line::from(kw_list.join(" ")));

        let mut dirs: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for t in &self.inline_tasks {
            let path_s = t.span.path.to_string_lossy().to_string();
            let comps: Vec<&str> = path_s.split('/').collect();
            if comps.len() >= 2 {
                let dir = format!("{}/{}", comps[0], comps[1]);
                *dirs.entry(dir).or_default() += 1;
            } else if let Some(name) = comps.first() {
                *dirs.entry(name.to_string()).or_default() += 1;
            }
        }
        let mut ds: Vec<(&String, &usize)> = dirs.iter().collect();
        ds.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        lines.push(Line::from(""));
        lines.push(Line::from("Top directories"));
        for (dir, count) in ds.iter().take(5) {
            lines.push(Line::from(format!("  {dir}/  [{count}]")));
        }

        let stale = self.inline_tasks.iter().filter(|t| t.is_stale(365)).count();
        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "Stale: {stale} / {}",
            self.inline_tasks.len()
        )));

        lines
    }

    /// Recompute the displayed inline tasks from the current filter, apply
    /// the current sort mode, then rebuild the tree/rows.
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
        self.sort_inline();
        self.rebuild_inline_rows();
        if !self.inline_rows.is_empty() {
            self.inline_selected = self.inline_selected.min(self.inline_rows.len() - 1);
        } else {
            self.inline_selected = 0;
        }
    }

    /// Sort `inline_displayed` by the current sort mode.
    fn sort_inline(&mut self) {
        match self.sort_mode {
            SortMode::Path => self.inline_displayed.sort_by(|a, b| {
                a.span
                    .path
                    .cmp(&b.span.path)
                    .then_with(|| a.span.line.cmp(&b.span.line))
            }),
            SortMode::Priority => self.inline_displayed.sort_by_key(|t| {
                let prio = match &t.metadata.priority {
                    Some(crate::model::Priority::High) => 0,
                    Some(crate::model::Priority::Med) => 1,
                    Some(crate::model::Priority::Low) => 2,
                    Some(crate::model::Priority::Other(_)) => 3,
                    None => 4,
                };
                (prio, t.span.path.clone(), t.span.line)
            }),
            SortMode::Keyword => self.inline_displayed.sort_by_key(|t| {
                let kw = match t.keyword.to_ascii_lowercase().as_str() {
                    "fixme" | "bug" => 0,
                    "hack" | "xxx" => 1,
                    "todo" => 2,
                    "note" => 3,
                    _ => 4,
                };
                (kw, t.span.path.clone(), t.span.line)
            }),
        }
    }

    /// `s`: cycle to the next sort mode and re-sort the displayed tasks.
    fn cycle_sort(&mut self) {
        self.sort_mode = self.sort_mode.next();
        self.sort_inline();
        self.inline_root = build_tree(&self.inline_displayed);
        self.inline_rows = flatten_inline(
            &self.inline_root,
            &self.inline_displayed,
            &self.expanded_inline,
            self.show_blame,
        );
        if !self.inline_rows.is_empty() {
            self.inline_selected = self.inline_selected.min(self.inline_rows.len() - 1);
        } else {
            self.inline_selected = 0;
        }
    }

    /// `g`: toggle blame annotations on inline task rows.
    fn toggle_blame(&mut self) {
        self.show_blame = !self.show_blame;
        self.goal_rows = flatten_goals(&self.goals, &self.goal_expanded, self.show_blame);
        self.inline_root = build_tree(&self.inline_displayed);
        self.inline_rows = flatten_inline(
            &self.inline_root,
            &self.inline_displayed,
            &self.expanded_inline,
            self.show_blame,
        );
        if !self.inline_rows.is_empty() {
            self.inline_selected = self.inline_selected.min(self.inline_rows.len() - 1);
        } else {
            self.inline_selected = 0;
        }
        if !self.goal_rows.is_empty() {
            self.goal_selected = self.goal_selected.min(self.goal_rows.len() - 1);
        } else {
            self.goal_selected = 0;
        }
    }

    /// Rebuild the inline tree/rows from the currently displayed tasks. Does
    /// not refilter; called after expand/collapse/toggle.
    fn rebuild_inline_rows(&mut self) {
        self.inline_root = build_tree(&self.inline_displayed);
        self.inline_rows = flatten_inline(
            &self.inline_root,
            &self.inline_displayed,
            &self.expanded_inline,
            self.show_blame,
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
        }
    }

    /// `Space` (goals view): toggle the selected item's completion in the
    /// source file and in memory. A checkbox item flips `[x]`/`[ ]` in place;
    /// a table row opens a cell-edit popup on its state cell. Goal headers and
    /// non-toggleable lines are left untouched.
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
        let lines: Vec<&str> = content.split('\n').collect();
        let idx = line.saturating_sub(1);
        let Some(row_line) = lines.get(idx) else {
            log::warn!("toggle: line {line} out of range in {}", abs.display());
            return;
        };

        // Checkbox item: flip in place.
        if let Some(new_line) = goals::flip_checkbox(row_line) {
            let mut all = lines.iter().map(|s| s.to_string()).collect::<Vec<_>>();
            all[idx] = new_line;
            if fs::write(&abs, all.join("\n")).is_err() {
                log::warn!("toggle: cannot write {}", abs.display());
                return;
            }
            if let Some(item) = goal_item_mut(&mut self.goals, gi, &path) {
                item.checked = !item.checked;
            }
            self.rebuild_active();
            return;
        }

        // Table row: open the state-cell edit popup.
        if let Some((state_col, value)) =
            crate::parser::goal::table_state_cell(&lines, line, &self.headers)
        {
            self.cell_input = value;
            self.cell_edit = Some(CellTarget {
                gi,
                path,
                line,
                state_col,
            });
            self.mode = Mode::CellEdit;
        } else {
            log::debug!(
                "toggle: not a checkbox or table row at {}:{}",
                abs.display(),
                line
            );
        }
    }

    /// Enter in cell-edit mode: write the edited value into the table cell and
    /// recompute the item's checked state.
    fn apply_cell_edit(&mut self) {
        let Some(target) = self.cell_edit.take() else {
            self.mode = Mode::Normal;
            return;
        };
        self.mode = Mode::Normal;
        let new_value = std::mem::take(&mut self.cell_input);

        let Some((rel, _)) = goal_item_span(&self.goals, target.gi, &target.path) else {
            return;
        };
        let line = target.line;
        let abs = self.root.join(&rel);
        let Ok(content) = fs::read_to_string(&abs) else {
            log::warn!("cell edit: cannot read {}", abs.display());
            return;
        };
        let mut all: Vec<String> = content.split('\n').map(String::from).collect();
        let idx = line.saturating_sub(1);
        if idx >= all.len() {
            log::warn!("cell edit: line {line} out of range");
            return;
        }
        all[idx] = crate::parser::goal::rewrite_state_cell(&all[idx], target.state_col, &new_value);
        if fs::write(&abs, all.join("\n")).is_err() {
            log::warn!("cell edit: cannot write {}", abs.display());
            return;
        }

        if let Some(item) = goal_item_mut(&mut self.goals, target.gi, &target.path) {
            item.checked = crate::parser::goal::done_heuristic(&new_value);
        }
        self.rebuild_active();
    }

    /// `e`: suspend the TUI, open the editor at the selected item's file and
    /// line, then resume. Resolution is delegated to the event loop.
    #[allow(clippy::bind_instead_of_map)]
    fn edit_selected(&mut self) {
        // Goals view: the selected item's span, or the goal's source file.
        let (rel, line) = match self.view {
            View::Goals => {
                if let Some(item_key) = self.selected_goal_item_key() {
                    if let Some((rel, line)) = parse_item_key(&item_key)
                        .and_then(|(gi, path)| goal_item_span(&self.goals, gi, &path))
                    {
                        (rel, line)
                    } else {
                        return;
                    }
                } else {
                    // Goal header: open its source file at line 1.
                    let Some(gi) =
                        self.goal_rows
                            .get(self.goal_selected)
                            .and_then(|r| match &r.kind {
                                GoalRowKind::Header { key, .. } => {
                                    key.strip_prefix('g').and_then(|s| s.parse::<usize>().ok())
                                }
                                _ => None,
                            })
                    else {
                        return;
                    };
                    let goal = &self.goals[gi];
                    (goal.source_file.clone(), 1)
                }
            }
            View::Inline => {
                let path = self
                    .inline_rows
                    .get(self.inline_selected)
                    .and_then(|r| match &r.kind {
                        inline_view::InlineRowKind::Dir(k)
                        | inline_view::InlineRowKind::File(k) => Some(k.clone()),
                        inline_view::InlineRowKind::Task { parent_key, line } => {
                            Some(format!("{parent_key}::{line}"))
                        }
                    });
                let Some(path) = path else {
                    return;
                };

                let line = if let Some((_, l)) = path.rsplit_once("::") {
                    l.parse::<usize>().unwrap_or(1)
                } else {
                    1
                };
                let rel_path = if let Some((p, _)) = path.rsplit_once("::") {
                    PathBuf::from(p)
                } else {
                    PathBuf::from(path)
                };
                (rel_path, line)
            }
        };
        self.pending_edit = Some((rel, line));
    }

    /// The key of the selected row in the active view. For a leaf, this is
    /// its parent's key, so expand/collapse/toggle act on the parent.
    fn current_key(&self) -> Option<String> {
        match self.view {
            View::Goals => self.selected_goal_key(),
            View::Inline => self.selected_inline_key(),
        }
    }

    /// Rebuild the active view's rows from its expand set, then re-anchor the
    /// cursor onto the same entry (or its nearest surviving ancestor) so the
    /// cursor never drifts across a rebuild.
    fn rebuild_active(&mut self) {
        let anchor = self.cursor_id();
        match self.view {
            View::Goals => {
                self.goal_rows = flatten_goals(&self.goals, &self.goal_expanded, self.show_blame);
            }
            View::Inline => self.rebuild_inline_rows(),
        }
        self.reanchor(anchor);
    }

    /// The stable identity of the selected row, used to track the cursor
    /// across rebuilds.
    fn cursor_id(&self) -> Option<String> {
        match self.view {
            View::Goals => self.goal_rows.get(self.goal_selected).map(goal_row_id),
            View::Inline => self
                .inline_rows
                .get(self.inline_selected)
                .map(inline_row_id),
        }
    }

    /// Move the cursor to the row whose id is `id`; if that row is gone (hidden
    /// by a collapse), walk up to the nearest surviving ancestor; if none,
    /// clamp into range.
    fn reanchor(&mut self, id: Option<String>) {
        let mut candidate = id;
        while let Some(cur) = candidate {
            let pos = match self.view {
                View::Goals => self.goal_rows.iter().position(|r| goal_row_id(r) == cur),
                View::Inline => self
                    .inline_rows
                    .iter()
                    .position(|r| inline_row_id(r) == cur),
            };
            if let Some(i) = pos {
                match self.view {
                    View::Goals => self.goal_selected = i,
                    View::Inline => self.inline_selected = i,
                }
                return;
            }
            candidate = ancestor_id(&cur);
        }
        self.clamp_active_cursor();
    }

    /// `C`: collapse every node whose subtree is fully complete -- a goal
    /// with status Completed, or a milestone that is itself checked and whose
    /// leaves are all checked. Hierarchical: intermediate nodes fold too.
    /// The cursor stays on its previous entry (or its nearest surviving node).
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
        self.rebuild_active();
    }

    /// `X`: expand every node in the active view.
    fn expand_all(&mut self) {
        let added: Vec<String> = match self.view {
            View::Goals => goals::all_node_keys(&self.goals),
            View::Inline => inline_view::all_node_keys(&self.inline_root),
        };
        let target = match self.view {
            View::Goals => &mut self.goal_expanded,
            View::Inline => &mut self.expanded_inline,
        };
        let mut changed = false;
        for key in added {
            if target.insert(key) {
                changed = true;
            }
        }
        if changed {
            self.rebuild_active();
        }
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
        match self.view {
            View::Goals => {
                if self.goal_expanded.contains(&key) {
                    self.goal_expanded.remove(&key);
                } else {
                    self.goal_expanded.insert(key);
                }
            }
            View::Inline => {
                if self.expanded_inline.contains(&key) {
                    self.expanded_inline.remove(&key);
                } else {
                    self.expanded_inline.insert(key);
                }
            }
        };
        self.rebuild_active();
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
                inline_view::InlineRowKind::Task { parent_key, .. } => parent_key.clone(),
            })
    }
}

/// Stable identity of a goals-view row (its own key).
fn goal_row_id(row: &GoalRow) -> String {
    match &row.kind {
        GoalRowKind::Header { key }
        | GoalRowKind::Milestone { key }
        | GoalRowKind::Task { key, .. } => key.clone(),
    }
}

/// Stable identity of an inline-view row: the dir/file key, or
/// `{file}::{line}` for a task (unique so the cursor can track a specific task).
fn inline_row_id(row: &InlineRow) -> String {
    match &row.kind {
        inline_view::InlineRowKind::Dir(k) | inline_view::InlineRowKind::File(k) => k.clone(),
        inline_view::InlineRowKind::Task { parent_key, line } => {
            format!("{parent_key}::{line}")
        }
    }
}

/// The parent identity of a row id, for re-anchoring when the row is hidden:
/// `g0/1/2` -> `g0/1`; `src/a.rs::42` -> `src/a.rs`; `src/a.rs` -> `src`.
fn ancestor_id(id: &str) -> Option<String> {
    if let Some((path, _line)) = id.rsplit_once("::") {
        return Some(path.to_string());
    }
    id.rsplit_once('/').map(|(parent, _)| parent.to_string())
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

/// Suspend and open an editor at `abs`:`line`. Resolves `$EDITOR` /
/// `$VISUAL`; falls back to `vi` (Unix) or `notepad` (Windows). On
/// non-Windows platforms `+{line}` is passed so the editor jumps to the
/// right spot.
fn editor_command(abs: &Path, _line: usize) -> std::process::Command {
    let editor = resolve_editor();
    let mut cmd = std::process::Command::new(&editor);
    #[cfg(not(windows))]
    {
        cmd.arg(format!("+{_line}"));
    }
    cmd.arg(abs);
    cmd
}

fn resolve_editor() -> String {
    if let Ok(e) = std::env::var("EDITOR") {
        if !e.is_empty() {
            return e;
        }
    }
    if let Ok(e) = std::env::var("VISUAL") {
        if !e.is_empty() {
            return e;
        }
    }
    #[cfg(windows)]
    {
        "notepad".to_string()
    }
    #[cfg(not(windows))]
    {
        "vi".to_string()
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
            blame_author: None,
            blame_date: None,
            blame_commit: None,
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
            blame_author: None,
            blame_date: None,
            blame_commit: None,
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

    // ---- cursor stability across row rebuilds ----

    fn goal(title: &str, items: Vec<GoalItem>) -> Goal {
        Goal {
            title: title.into(),
            source_file: PathBuf::from("x.md"),
            badge: "(root)".into(),
            items,
        }
    }

    /// goalA: active (milestone m1 with leaves a unchecked + b checked, plus
    /// leaf c). goalB: completed (single checked leaf d).
    fn sample_goals() -> Vec<Goal> {
        let m1 = milestone("m1", false, vec![leaf("a", false), leaf("b", true)]);
        let goal_a = goal("A", vec![m1, leaf("c", false)]);
        let goal_b = goal("B", vec![leaf("d", true)]); // 100% -> Completed
        vec![goal_a, goal_b]
    }

    fn select_goal(app: &mut App, id: &str) {
        app.rebuild_active();
        let i = app
            .goal_rows
            .iter()
            .position(|r| goal_row_id(r) == id)
            .unwrap();
        app.goal_selected = i;
    }

    fn selected_goal_id(app: &App) -> String {
        goal_row_id(&app.goal_rows[app.goal_selected])
    }

    #[test]
    fn ancestor_id_walks_up() {
        assert_eq!(ancestor_id("g0/1/2"), Some("g0/1".to_string()));
        assert_eq!(ancestor_id("g0"), None);
        assert_eq!(ancestor_id("src/a.rs::42"), Some("src/a.rs".to_string()));
        assert_eq!(ancestor_id("src/a.rs"), Some("src".to_string()));
        assert_eq!(ancestor_id("src"), None);
    }

    #[test]
    fn expand_all_keeps_cursor_on_later_goal() {
        let mut app = App::new(sample_goals(), vec![], PathBuf::from("."), HashMap::new());
        select_goal(&mut app, "g1"); // cursor on goalB header
        app.expand_all(); // X: expands everything above; index would drift
        assert_eq!(selected_goal_id(&app), "g1");
    }

    #[test]
    fn collapse_all_moves_cursor_to_ancestor() {
        let mut app = App::new(sample_goals(), vec![], PathBuf::from("."), HashMap::new());
        app.goal_expanded.insert("g0".into());
        app.goal_expanded.insert("g0/0".into());
        select_goal(&mut app, "g0/0/0"); // cursor on leaf "a"
        app.collapse_all(); // Z
        assert_eq!(selected_goal_id(&app), "g0"); // leaf gone -> goal header
    }

    #[test]
    fn collapse_leaf_moves_cursor_to_parent() {
        let mut app = App::new(sample_goals(), vec![], PathBuf::from("."), HashMap::new());
        app.goal_expanded.insert("g0".into());
        app.goal_expanded.insert("g0/0".into());
        select_goal(&mut app, "g0/0/0"); // leaf "a"
        app.collapse_selected(); // h: folds parent milestone m1
        assert_eq!(selected_goal_id(&app), "g0/0");
    }

    #[test]
    fn expand_node_keeps_cursor() {
        let mut app = App::new(sample_goals(), vec![], PathBuf::from("."), HashMap::new());
        select_goal(&mut app, "g0"); // goalA header, collapsed
        app.expand_selected(); // l
        assert_eq!(selected_goal_id(&app), "g0");
    }

    #[test]
    fn toggle_leaf_moves_cursor_to_parent() {
        let mut app = App::new(sample_goals(), vec![], PathBuf::from("."), HashMap::new());
        app.goal_expanded.insert("g0".into());
        app.goal_expanded.insert("g0/0".into());
        select_goal(&mut app, "g0/0/0"); // leaf "a"
        app.toggle_selected(); // Enter: collapses parent m1
        assert_eq!(selected_goal_id(&app), "g0/0");
    }

    #[test]
    fn collapse_completed_reanchors_to_ancestor() {
        let mut app = App::new(sample_goals(), vec![], PathBuf::from("."), HashMap::new());
        app.goal_expanded.insert("g1".into()); // expand completed goalB
        select_goal(&mut app, "g1/0"); // leaf "d" under goalB
        app.collapse_completed(); // C: collapses goalB
        assert_eq!(selected_goal_id(&app), "g1"); // d gone -> goalB header
    }

    fn itask(path: &str, line: usize) -> InlineTask {
        InlineTask {
            keyword: "TODO".into(),
            scope: None,
            description: "x".into(),
            metadata: Metadata::default(),
            span: Span {
                path: PathBuf::from(path),
                line,
            },
            blame_author: None,
            blame_date: None,
            blame_commit: None,
        }
    }

    #[test]
    fn expand_all_inline_keeps_cursor() {
        let tasks = vec![itask("a/x.rs", 1), itask("b/y.rs", 2)];
        let mut app = App::new(vec![], tasks, PathBuf::from("."), HashMap::new());
        app.view = View::Inline;
        app.rebuild_active();
        // cursor on "b/" (index would drift as "a/" expands above it)
        app.inline_selected = app
            .inline_rows
            .iter()
            .position(|r| inline_row_id(r) == "b")
            .unwrap();
        app.expand_all();
        assert_eq!(inline_row_id(&app.inline_rows[app.inline_selected]), "b");
    }

    #[test]
    fn toggling_blame_rebuilds_rows() {
        let mut app = App::new(sample_goals(), vec![], PathBuf::from("."), HashMap::new());
        assert!(!app.show_blame);
        app.toggle_blame();
        assert!(app.show_blame);
        assert!(!app.goal_rows.is_empty());
        assert!(app.goal_selected < app.goal_rows.len());
        app.toggle_blame();
        assert!(!app.show_blame);
        assert!(!app.goal_rows.is_empty());
    }
}
