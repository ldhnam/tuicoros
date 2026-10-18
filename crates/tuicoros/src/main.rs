//! tuicoros — a terminal UI for COROS Training Hub.
//!
//! Start with `docs/ARCHITECTURE.md` for how the pieces fit together.

mod action;
mod app;
mod cli;
mod commands;
mod config;
mod event;
mod format;
mod logging;
mod paths;
mod status;
mod storage;
mod theme;
mod tui;

use std::time::Duration;

use clap::Parser;
use color_eyre::Result;

use crate::app::App;
use crate::cli::{Cli, Command};
use crate::commands::Ctx;
use crate::config::LoadedConfig;
use crate::event::Events;
use crate::paths::Paths;
use crate::theme::Theme;

/// How often a `Tick` event fires. Used for spinners later; idle ticks do not redraw.
const TICK_RATE: Duration = Duration::from_millis(250);

#[tokio::main]
async fn main() -> Result<()> {
    tui::install_hooks()?;
    let cli = Cli::parse();

    let env = |key: &str| std::env::var(key).ok();
    let paths = Paths::resolve(&env)?;
    let config_path = cli.config.clone().unwrap_or_else(|| paths.config_file());
    let mut loaded = config::load(&config_path, &env)?;
    if let Some(region) = cli.region {
        loaded.config.region = region;
        loaded.region_explicit = true;
    }

    // Logging failures are not fatal: the app still works without a log file.
    let _log_guard = match logging::init(&paths.log_dir(), &loaded.config.log.level) {
        Ok(guard) => Some(guard),
        Err(err) => {
            eprintln!("warning: file logging disabled: {err:#}");
            None
        }
    };
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        region = %loaded.config.region,
        config = %loaded.path.display(),
        "starting"
    );
    for warning in &loaded.warnings {
        tracing::warn!("config: {warning}");
    }

    let ctx = Ctx {
        paths: &paths,
        loaded: &loaded,
        cli_region: cli.region,
    };
    match cli.command.clone().unwrap_or_default() {
        Command::Tui => run_tui(&loaded).await,
        Command::Status => {
            let vault = ctx.vault();
            let session = status::SessionInfo {
                store: vault.describe(),
                session: vault.session()?,
                remembered: vault.credentials()?.is_some(),
            };
            status::write_status(&mut std::io::stdout().lock(), &paths, &loaded, &session)?;
            Ok(())
        }
        Command::Login(args) => commands::login::run(&ctx, &args).await,
        Command::Logout => commands::logout(&ctx),
        Command::Api(command) => commands::api::run(&ctx, &command).await,
    }
}

async fn run_tui(loaded: &LoadedConfig) -> Result<()> {
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let theme = Theme::new(loaded.config.display.theme, no_color);
    let notice = loaded.warnings.first().map(|w| format!("config: {w}"));
    let app = App::new(loaded.config.region, theme, notice);

    let mut terminal = tui::init()?;
    let events = Events::start(TICK_RATE);
    let result = app.run(&mut terminal, events).await;
    tui::restore();
    tracing::info!(ok = result.is_ok(), "exiting");
    result
}
