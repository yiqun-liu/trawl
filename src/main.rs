//! trawl binary entry point.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use trawl::{scan, Config, InlineTask, ParseContext, Priority, ScanOptions, ScanResult, Status};

/// TODO Repository Annotation Work List.
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    let config = Config::load(&cli.path)?;
    log::debug!("loaded config: {} keywords", config.scan.keywords.len());

    let options = ScanOptions::from_config(cli.path.clone(), &config)?;
    let ctx = ParseContext::from_config(&config)?;
    let result = scan(&options, &ctx)?;

    if cli.no_tui {
        print_summary(&result);
    } else {
        trawl::tui::run(result)?;
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

/// Initialize the logger. `verbose` selects `debug`, otherwise `warn`.
fn init_logging(verbose: bool) {
    let level = if verbose { "debug" } else { "warn" };
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level))
        .format_target(false)
        .try_init();
}
