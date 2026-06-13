//! trawl binary entry point.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use trawl::Config;

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    init_logging(cli.verbose);

    let config = Config::load(&cli.path)?;
    log::debug!("loaded config: {} keywords", config.scan.keywords.len());

    // The scanner and parsers arrive in later slices. For now, report that
    // configuration loaded successfully so the CLI is exercised end to end.
    eprintln!(
        "trawl: ready ({} keywords, {} goal section names)",
        config.scan.keywords.len(),
        config.scan.goal_section_names.len()
    );
    Ok(())
}

/// Initialize the logger. `verbose` selects `debug`, otherwise `warn`.
fn init_logging(verbose: bool) {
    let level = if verbose { "debug" } else { "warn" };
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level))
        .format_target(false)
        .try_init();
}
