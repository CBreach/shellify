mod app;
mod command;
mod config;
mod keymap;
mod player;
mod provider;
mod ui;

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use directories::ProjectDirs;
use tracing_appender::non_blocking::WorkerGuard;

use crate::app::App;
use crate::config::Config;

#[derive(Parser)]
#[command(
    name = "shellify",
    version,
    about = "Listen to music from your terminal"
)]
struct Cli {
    /// Path to the config file (default: ~/.config/shellify/config.toml)
    #[arg(long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let _log_guard = init_logging()?;

    // Load config before touching the terminal so errors print normally.
    let config_path = cli.config.unwrap_or_else(config::default_path);
    let config = Config::load(&config_path)?;
    let app = App::new(&config, config_path)?;

    let terminal = ratatui::init();
    // ratatui's panic hook restores raw mode and the screen, but not mouse
    // capture (which we may enable), so release that first on a panic.
    let restore_terminal = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        app::set_mouse_capture(false);
        restore_terminal(info);
    }));
    let result = app.run(terminal).await;
    ratatui::restore();
    result
}

/// The TUI owns stdout, so logs go to a file in the cache dir.
fn init_logging() -> Result<WorkerGuard> {
    let dirs =
        ProjectDirs::from("", "", "shellify").context("could not determine home directory")?;
    let log_dir = dirs.cache_dir();
    std::fs::create_dir_all(log_dir)
        .with_context(|| format!("creating log dir {}", log_dir.display()))?;

    let (writer, guard) =
        tracing_appender::non_blocking(tracing_appender::rolling::never(log_dir, "shellify.log"));
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_env("SHELLIFY_LOG")
                .unwrap_or_else(|_| "shellify=info".into()),
        )
        .init();
    Ok(guard)
}
