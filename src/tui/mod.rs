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

use crate::model::{Goal, InlineTask};
use crate::ScanResult;

mod goals;
mod inline_view;

use goals::{flatten_goals, GoalRow};

type Tui = Terminal<CrosstermBackend<Stdout>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Goals,
    Inline,
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
    // Ctrl+C always quits.
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.quit = true;
        return;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => app.quit = true,
        KeyCode::Tab => app.toggle_view(),
        KeyCode::Char('j') | KeyCode::Down => app.move_cursor(1),
        KeyCode::Char('k') | KeyCode::Up => app.move_cursor(-1),
        KeyCode::Char('l') | KeyCode::Enter => app.expand_selected(),
        KeyCode::Char('h') | KeyCode::Backspace => app.collapse_selected(),
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

    let hint = match app.view {
        View::Goals => "Enter/l: expand  h: collapse  j/k: move  Tab: Inline Tasks  q: quit",
        View::Inline => "Tab: Goals  q: quit",
    };
    let footer_widget =
        Paragraph::new(Line::from(hint)).style(Style::default().add_modifier(Modifier::REVERSED));
    f.render_widget(footer_widget, footer);
}

/// Application state for the TUI.
struct App {
    goals: Vec<Goal>,
    inline_tasks: Vec<InlineTask>,
    view: View,
    goal_rows: Vec<GoalRow>,
    goal_selected: usize,
    expanded_goals: HashSet<usize>,
    quit: bool,
}

impl App {
    fn new(goals: Vec<Goal>, inline_tasks: Vec<InlineTask>) -> Self {
        let expanded_goals = HashSet::new();
        let goal_rows = flatten_goals(&goals, &expanded_goals);
        Self {
            goals,
            inline_tasks,
            view: View::Goals,
            goal_rows,
            goal_selected: 0,
            expanded_goals,
            quit: false,
        }
    }

    fn toggle_view(&mut self) {
        self.view = match self.view {
            View::Goals => View::Inline,
            View::Inline => View::Goals,
        };
    }

    fn move_cursor(&mut self, delta: i32) {
        let View::Goals = self.view else {
            return; // inline view navigation arrives in a later slice
        };
        let len = self.goal_rows.len();
        if len == 0 {
            return;
        }
        let next = (self.goal_selected as i32 + delta).clamp(0, (len - 1) as i32);
        self.goal_selected = next as usize;
    }

    fn expand_selected(&mut self) {
        if self.view != View::Goals {
            return;
        }
        if let Some(gi) = self.selected_goal_header() {
            if self.expanded_goals.insert(gi) {
                self.goal_rows = flatten_goals(&self.goals, &self.expanded_goals);
            }
        }
    }

    fn collapse_selected(&mut self) {
        if self.view != View::Goals {
            return;
        }
        if let Some(gi) = self.selected_goal_header() {
            if self.expanded_goals.remove(&gi) {
                self.goal_rows = flatten_goals(&self.goals, &self.expanded_goals);
            }
        }
    }

    /// If the selected row is a goal header, return its goal index.
    fn selected_goal_header(&self) -> Option<usize> {
        self.goal_rows
            .get(self.goal_selected)
            .and_then(|row| match row.kind {
                goals::GoalRowKind::Header(gi) => Some(gi),
                goals::GoalRowKind::Item => None,
            })
    }
}
