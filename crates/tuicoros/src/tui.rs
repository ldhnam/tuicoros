//! Terminal setup and teardown. The one rule: whatever happens (normal exit,
//! error, panic), the user's shell must come back in a usable state.

use std::sync::atomic::{AtomicBool, Ordering};

use color_eyre::Result;
use color_eyre::config::HookBuilder;
use ratatui::DefaultTerminal;

/// True while raw mode + alternate screen are on, so `restore` only touches the
/// terminal when needed (e.g. not for `tuicoros status` output).
static ACTIVE: AtomicBool = AtomicBool::new(false);

/// Install panic and error hooks that restore the terminal before printing.
/// Call once, first thing in `main`.
pub fn install_hooks() -> Result<()> {
    let (panic_hook, eyre_hook) = HookBuilder::default()
        .panic_section("This is a bug in tuicoros. Please report it and attach the log file (see `tuicoros status`).")
        .display_location_section(false)
        .into_hooks();
    eyre_hook.install()?;

    let panic_hook = panic_hook.into_panic_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        tracing::error!(panic = %info, "panic");
        panic_hook(info);
    }));
    Ok(())
}

/// Enter raw mode and the alternate screen.
pub fn init() -> Result<DefaultTerminal> {
    let terminal = ratatui::try_init()?;
    ACTIVE.store(true, Ordering::SeqCst);
    Ok(terminal)
}

/// Leave raw mode and the alternate screen. Safe to call more than once.
pub fn restore() {
    if ACTIVE.swap(false, Ordering::SeqCst)
        && let Err(err) = ratatui::try_restore()
    {
        eprintln!("failed to restore the terminal: {err}");
    }
}
