//! tuicoros — a terminal UI for COROS Training Hub.
//!
//! Start with `docs/ARCHITECTURE.md` for how the pieces fit together.

mod action;
mod app;
mod charts;
mod cli;
mod commands;
mod config;
mod event;
mod format;
mod keymap;
mod logging;
mod paths;
mod screens;
mod service;
mod status;
mod storage;
#[cfg(test)]
mod testdata;
mod theme;
mod tui;
mod widgets;

use std::time::Duration;

use clap::Parser;
use color_eyre::Result;

use chrono::{Local, Utc};

use crate::app::{App, AppOptions};
use crate::cli::{CacheCommand, Cli, Command};
use crate::commands::Ctx;
use crate::event::Events;
use crate::paths::Paths;
use crate::service::DataService;
use crate::theme::Theme;

/// How often a `Tick` event fires (spinners, toasts). Idle ticks do not redraw.
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
        Command::Tui => run_tui(&ctx).await,
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
        Command::Logout { purge } => commands::logout(&ctx, purge),
        Command::Cache(CacheCommand::Clear) => {
            let freed = storage::cache::clear(&paths.cache_dir)?;
            println!("Cache cleared ({} KB freed).", freed / 1024);
            Ok(())
        }
        Command::Api(command) => commands::api::run(&ctx, &command).await,
    }
}

async fn run_tui(ctx: &Ctx<'_>) -> Result<()> {
    let loaded = ctx.loaded;
    let config = &loaded.config;
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let theme = Theme::new(config.display.theme, no_color);

    let vault = ctx.vault();
    let stored = vault.session().unwrap_or_else(|err| {
        tracing::warn!(error = %format!("{err:#}"), "cannot read the stored session");
        None
    });
    let options = AppOptions {
        region: config.region,
        page_size: u32::from(config.activities.page_size),
        default_range_days: config.activities.default_range_days,
        analysis_weeks: u32::from(config.analysis.default_weeks),
        notice: loaded.warnings.first().map(|w| format!("config: {w}")),
        last_email: stored.as_ref().and_then(|s| s.email.clone()),
    };

    let (results_tx, results_rx) = tokio::sync::mpsc::unbounded_channel();
    let network = config.network.clone();
    let service = DataService::new(
        results_tx,
        vault,
        ctx.paths.cache_dir.clone(),
        move |region| commands::client_config(&network, region),
    );
    let mut app = App::new(theme, options, Local::now().date_naive(), Utc::now());
    if let Some(stored) = stored {
        match service.resume(&stored) {
            Ok(()) => {
                for command in app.signed_in(stored.short_name(), stored.region) {
                    service.dispatch(command);
                }
            }
            Err(err) => {
                tracing::warn!(error = %format!("{err:#}"), "cannot resume the stored session")
            }
        }
    }

    let mut terminal = tui::init()?;
    let events = Events::start(TICK_RATE);
    let result = app.run(&mut terminal, events, results_rx, service).await;
    tui::restore();
    tracing::info!(ok = result.is_ok(), "exiting");
    result
}
