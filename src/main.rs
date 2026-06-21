//! trawl binary entry point.

use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;

use trawl::{scan, Config, InlineTask, ParseContext, Priority, ScanOptions, ScanResult, Status};

/// `TODO` Repository Annotation Work List.
///
/// Discover and visualize work items (inline TODOs and goal trackers)
/// embedded in a repository.
#[derive(Debug, Parser)]
#[command(
    name = "trawl",
    version,
    about = "TODO Repository Annotation Work List"
)]
struct Cli {
    /// Repository path to scan.
    #[arg(long, default_value = ".")]
    path: PathBuf,

    /// Enable verbose (debug) logging.
    #[arg(long)]
    verbose: bool,

    /// Skip the TUI and print a summary instead (for scripts / no-TTY contexts).
    #[arg(long)]
    no_tui: bool,

    /// Write logs to <PATH> instead of the platform-conventional location.
    #[arg(long, value_name = "PATH")]
    log_file: Option<PathBuf>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose, cli.log_file);

    let config = Config::load(&cli.path)?;
    log::debug!("loaded config: {} keywords", config.scan.keywords.len());

    let options = ScanOptions::from_config(cli.path.clone(), &config)?;
    let ctx = ParseContext::from_config(&config)?;
    let result = scan(&options, &ctx)?;

    if cli.no_tui {
        print_summary(&result);
    } else {
        trawl::tui::run(result, cli.path.clone(), config.headers.clone())?;
    }
    Ok(())
}

/// Print a concise summary of the scan to stdout (the TUI replaces this later).
fn print_summary(result: &ScanResult) {
    println!("goals: {}", result.goals.len());

    for goal in &result.goals {
        let pct = (goal.progress() * 100.0).round() as u32;
        println!(
            "  [{:>8}] {:>3}%  {}  —  {}",
            status_label(goal.status()),
            pct,
            goal.title,
            goal.badge
        );
    }

    let (high, med, low, other, untagged) = priority_breakdown(&result.inline_tasks);
    println!(
        "inline tasks: {}  (high:{} med:{} low:{} other:{} untagged:{})",
        result.inline_tasks.len(),
        high,
        med,
        low,
        other,
        untagged
    );
    for task in &result.inline_tasks {
        let scope = task
            .scope
            .as_deref()
            .map(|s| format!("({s})"))
            .unwrap_or_default();
        println!(
            "  {}:{}  {}{}  {}",
            task.span.path.display(),
            task.span.line,
            task.keyword,
            scope,
            task.description
        );
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::Planned => "planned",
        Status::Active => "active",
        Status::Completed => "done",
    }
}

fn priority_breakdown(tasks: &[InlineTask]) -> (usize, usize, usize, usize, usize) {
    let (mut high, mut med, mut low, mut other, mut untagged) = (0, 0, 0, 0, 0);
    for t in tasks {
        match &t.metadata.priority {
            Some(Priority::High) => high += 1,
            Some(Priority::Med) => med += 1,
            Some(Priority::Low) => low += 1,
            Some(Priority::Other(_)) => other += 1,
            None => untagged += 1,
        }
    }
    (high, med, low, other, untagged)
}

/// Initialize the logger. `verbose` selects `debug`, otherwise `warn`. Logs
/// go to `log_file` if given, else the platform-conventional location; if the
/// file cannot be opened, fall back to stderr.
fn init_logging(verbose: bool, log_file: Option<PathBuf>) {
    let level = if verbose { "debug" } else { "warn" };
    let path = log_file.unwrap_or_else(conventional_log_path);
    let target = match open_log(&path) {
        Some(file) => env_logger::Target::Pipe(Box::new(file)),
        None => env_logger::Target::Stderr,
    };
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level))
        .target(target)
        .format_target(false)
        .try_init();
}

/// Open (creating parents and the file) the log file for appending.
fn open_log(path: &Path) -> Option<File> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    OpenOptions::new().create(true).append(true).open(path).ok()
}

/// Platform-conventional default log path (never the terminal, so the TUI is
/// never corrupted). Linux: XDG state dir; macOS: ~/Library/Logs; Windows:
/// %LOCALAPPDATA%.
fn conventional_log_path() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(base)
            .join("trawl")
            .join("logs")
            .join("trawl.log")
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join("Library/Logs/trawl/trawl.log")
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(xdg) = std::env::var("XDG_STATE_HOME") {
            PathBuf::from(xdg).join("trawl/trawl.log")
        } else {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(home).join(".local/state/trawl/trawl.log")
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        PathBuf::from("trawl.log")
    }
}
