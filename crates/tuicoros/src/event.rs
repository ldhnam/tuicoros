//! Terminal input, ticks and shutdown signals merged into one stream of `Event`s.

use std::time::Duration;

use crossterm::event::{Event as TermEvent, EventStream, KeyEvent};
use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Key(KeyEvent),
    Resize(u16, u16),
    /// Fires every tick; used for animations. Does not redraw by itself.
    Tick,
    /// The process was asked to stop (SIGTERM / SIGHUP) or the terminal went away.
    Terminate,
}

/// Background task reading terminal events. Stops when dropped.
pub struct Events {
    rx: mpsc::UnboundedReceiver<Event>,
    cancel: CancellationToken,
}

impl Events {
    pub fn start(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let cancel = CancellationToken::new();
        tokio::spawn(pump(tx, cancel.clone(), tick_rate));
        Self { rx, cancel }
    }

    /// The next event. If the reader task is gone, that means `Terminate`.
    pub async fn next(&mut self) -> Event {
        self.rx.recv().await.unwrap_or(Event::Terminate)
    }
}

impl Drop for Events {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

async fn pump(tx: mpsc::UnboundedSender<Event>, cancel: CancellationToken, tick_rate: Duration) {
    let mut terminal_events = EventStream::new();
    let mut tick = tokio::time::interval(tick_rate);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut signals = ShutdownSignals::new();

    loop {
        let event = tokio::select! {
            () = cancel.cancelled() => break,
            () = signals.recv() => Event::Terminate,
            _ = tick.tick() => Event::Tick,
            next = terminal_events.next() => match next {
                Some(Ok(TermEvent::Key(key))) => Event::Key(key),
                Some(Ok(TermEvent::Resize(w, h))) => Event::Resize(w, h),
                Some(Ok(_)) => continue, // mouse, focus, paste: not used yet
                Some(Err(err)) => {
                    tracing::error!(%err, "reading terminal events failed");
                    Event::Terminate
                }
                None => Event::Terminate,
            },
        };
        let stop = event == Event::Terminate;
        if tx.send(event).is_err() || stop {
            break;
        }
    }
}

/// SIGTERM and SIGHUP on Unix. Never fires elsewhere (Ctrl+C arrives as a key in raw mode).
struct ShutdownSignals {
    #[cfg(unix)]
    signals: Vec<tokio::signal::unix::Signal>,
}

impl ShutdownSignals {
    fn new() -> Self {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            let signals = [SignalKind::terminate(), SignalKind::hangup()]
                .into_iter()
                .filter_map(|kind| match signal(kind) {
                    Ok(s) => Some(s),
                    Err(err) => {
                        tracing::warn!(%err, "cannot listen for signal");
                        None
                    }
                })
                .collect();
            Self { signals }
        }
        #[cfg(not(unix))]
        Self {}
    }

    async fn recv(&mut self) {
        #[cfg(unix)]
        if !self.signals.is_empty() {
            let waits = self.signals.iter_mut().map(|s| Box::pin(s.recv()));
            let _ = futures::future::select_all(waits).await;
            return;
        }
        std::future::pending::<()>().await;
    }
}
