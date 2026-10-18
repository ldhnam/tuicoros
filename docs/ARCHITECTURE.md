# Architecture

This describes how `tuicoros` is put together. The *why* behind each choice lives in
[DECISIONS.md](DECISIONS.md). API details live in [COROS_API.md](COROS_API.md).

## 1. Goals and non-goals

**Goals**

- Browse COROS Training Hub data from the terminal: activities, activity details (laps, charts,
  map), EvoLab metrics (training load, fatigue, HRV, RHR), training calendar and workout library.
- Be fast and pleasant: instant startup from cache, no frozen UI while the network works, full
  keyboard control.
- Treat the private API politely: cache, limit concurrency, never hammer login.
- Keep credentials safe: OS keychain, no secrets in logs.

**Non-goals (for v1)**

- Editing workouts or plans, uploading FIT files, deleting activities (possible later, behind
  confirmations).
- Sleep stages via the reverse-engineered mobile API.
- Team / coach features of the Training Hub.

## 2. Workspace layout

A Cargo workspace with two crates. The API library knows nothing about the terminal, so it can be
tested on its own and reused later (e.g. by a CLI or another frontend).

```
tuicoros/
├── Cargo.toml                    # [workspace], shared lints, shared dependency versions
├── rust-toolchain.toml           # pin stable
├── crates/
│   ├── coros-api/                # library: HTTP client + models. No ratatui/crossterm.
│   │   ├── src/
│   │   │   ├── lib.rs
│   │   │   ├── client.rs         # CorosClient: base URL, headers, retry, re-login, semaphore
│   │   │   ├── region.rs         # Region enum → base URL
│   │   │   ├── auth.rs           # Credentials, md5 password, Session { token, user_id, region }
│   │   │   ├── envelope.rs       # { result, message, apiCode, tlogId, data } decoding
│   │   │   ├── error.rs          # CorosError (thiserror)
│   │   │   ├── endpoints/        # one module per API area; thin functions
│   │   │   │   ├── account.rs
│   │   │   │   ├── activity.rs
│   │   │   │   ├── analyse.rs
│   │   │   │   ├── dashboard.rs
│   │   │   │   └── training.rs
│   │   │   ├── raw/              # serde mirrors of the JSON, tolerant (#[serde(default)])
│   │   │   ├── model/            # clean domain types with real units (see §5)
│   │   │   ├── units.rs          # centi-scale, quarter-hour tz, YYYYMMDD, GPS 1e7, pace
│   │   │   └── sport.rs          # SportType enum with Unknown(u32)
│   │   └── tests/
│   │       ├── fixtures/         # scrubbed real JSON responses
│   │       └── *.rs              # wiremock + fixture tests
│   └── tuicoros/                 # binary
│       └── src/
│           ├── main.rs           # clap parse → setup logging, config → run subcommand
│           ├── cli.rs            # subcommands: (default) tui, login, logout, status, api …
│           ├── tui.rs            # terminal enter/restore, panic + error hooks
│           ├── event.rs          # EventStream + tick + render timer → Event
│           ├── action.rs         # Action enum (the only way state changes)
│           ├── app.rs            # App: state, update(action), render dispatch
│           ├── service.rs        # DataService: runs API calls in tasks, talks to cache
│           ├── storage/
│           │   ├── config.rs     # config.toml
│           │   ├── secrets.rs    # keychain access (password hash, token)
│           │   └── cache.rs      # on-disk cache (JSON files in v1, SQLite later)
│           ├── screens/          # one module per screen (see UI_DESIGN.md)
│           │   ├── login.rs
│           │   ├── dashboard.rs
│           │   ├── activities.rs
│           │   ├── activity_detail.rs
│           │   ├── analysis.rs
│           │   ├── calendar.rs
│           │   ├── workouts.rs
│           │   └── help.rs
│           ├── widgets/          # reusable pieces: stat tile, zone bar, line chart, calendar grid, track map
│           ├── format.rs         # pace, duration, distance, units (metric/imperial)
│           ├── keymap.rs         # key → Action per screen; help text generated from it
│           └── theme.rs          # colours, sport colours, light/dark
├── xtask/                        # dev tooling: `cargo run -p xtask -- scrub <file>` for fixtures
├── docs/
└── .github/workflows/ci.yml
```

## 3. Runtime model

The app uses **unidirectional data flow**, a simplified Elm architecture:

1. Something happens: a key press, a tick, or a network result.
2. It becomes an `Action`.
3. `App::update(action)` changes state. It is **synchronous and does no I/O**. It may return
   `Command`s (e.g. "fetch activities page 2").
4. Commands go to the `DataService`, which runs them on tokio tasks and sends results back as
   `Action`s through an unbounded channel.
5. The UI re-renders from state when something changed.

```
                ┌───────────────────────── tokio main task ─────────────────────────┐
 crossterm      │                                                                   │
 EventStream ──►│  select! {                                                        │
 tick (250ms) ─►│    ev  = events.next()   => screen.handle_event(ev) -> Action     │
 action_rx ────►│    act = action_rx.recv() => act                                  │
                │  }                                                                │
                │  let cmds = app.update(action);   // pure state change            │
                │  for c in cmds { service.dispatch(c) }                            │
                │  if app.dirty { terminal.draw(|f| app.render(f)) }                │
                └───────────────────────────────────────────────────────────────────┘
                                      │ Command                 ▲ Action::Loaded{..}
                                      ▼                         │
                ┌──────────── DataService (spawned tasks) ──────┴───────────┐
                │  cache.get() ─hit──► send Loaded(stale=true?)             │
                │      └─miss/stale─► CorosClient ─► HTTPS ─► cache.put()   │
                └───────────────────────────────────────────────────────────┘
```

Key rules:

- **Never `await` the network in the UI loop.** All I/O goes through `DataService`.
- **Stale responses are dropped.** Each request carries a `RequestId`. A screen remembers the id
  it is waiting for and ignores older results (e.g. the user changed the filter while page 1 was
  loading).
- **Cancellation.** Tasks get a `CancellationToken` (from `tokio-util`). Leaving a screen cancels
  its in-flight detail fetches.
- **Render on change**, plus at most ~30 fps while a spinner is animating. Idle means no redraws.

### Loading state

Every remote value on screen is wrapped in:

```rust
enum Remote<T> {
    NotAsked,
    Loading { previous: Option<T> },  // keep showing old data while refreshing
    Ready { value: T, fetched_at: DateTime<Utc>, from_cache: bool },
    Failed { error: UiError, previous: Option<T> },
}
```

Screens render all four states. The status bar shows "cached 3 h ago · r to refresh" when relevant.

### Screens

```rust
trait Screen {
    fn id(&self) -> ScreenId;
    fn on_enter(&mut self, ctx: &Ctx) -> Vec<Command>;          // start fetches
    fn handle_key(&mut self, key: KeyEvent, ctx: &Ctx) -> Option<Action>;
    fn update(&mut self, action: &Action, ctx: &Ctx) -> Vec<Command>;
    fn render(&self, frame: &mut Frame, area: Rect, ctx: &Ctx);
    fn keymap(&self) -> &'static [KeyHint];                       // drives footer + help
}
```

`App` holds a navigation stack (`Vec<ScreenId>`) for drill-down (Activities → Activity detail), plus
top-level tabs. Global keys (quit, help, tab switch) are handled by `App` before the screen sees them.

## 4. API client (`coros-api`)

```rust
pub struct CorosClient { http: reqwest::Client, region: Region, session: RwLock<Option<Session>>,
                         reauth: Option<Arc<dyn Reauth>>, limiter: Semaphore }

impl CorosClient {
    pub async fn login(&self, creds: &Credentials) -> Result<Session>;
    pub async fn account(&self) -> Result<Account>;
    pub async fn activities(&self, q: &ActivityQuery) -> Result<Page<ActivitySummary>>;
    pub async fn activity_detail(&self, id: &LabelId, sport: SportType) -> Result<ActivityDetail>;
    pub async fn activity_download_url(&self, id: &LabelId, sport: SportType, fmt: FileFormat) -> Result<Url>;
    pub async fn analyse(&self, range: DateRange) -> Result<Analyse>;
    pub async fn day_detail(&self, range: DateRange) -> Result<Vec<DailyMetrics>>;
    pub async fn dashboard(&self) -> Result<Dashboard>;
    pub async fn schedule(&self, range: DateRange) -> Result<Schedule>;
    pub async fn workouts(&self) -> Result<Vec<Workout>>;
}
```

- **One request path** (`send<T>()`) does: acquire semaphore → add headers → send → read text →
  decode envelope → map errors → on `1019` call `Reauth` once and retry if the request is idempotent.
- `Reauth` is a trait the binary implements. It loads the stored password hash from the keychain and
  logs in again. If no credentials are stored, the error surfaces and the UI shows the login screen.
- Errors:

```rust
pub enum CorosError {
    Api { result: String, message: String, tlog_id: Option<String> },  // result != "0000"
    TokenInvalid { region: Region },                                   // 1019 after re-auth
    RateLimited { retry_after: Option<Duration> },                     // 429 or HTML body
    Http { status: StatusCode, body_snippet: String },
    Network(reqwest::Error),
    Decode { endpoint: &'static str, source: serde_json::Error, body_snippet: String },
    NotAuthenticated,
}
```

- `Decode` errors keep the endpoint name and the first 500 bytes of the body (scrubbed) so schema
  drift is easy to debug from logs.

## 5. Two-layer models: `raw` and `model`

The API's JSON is messy (zero-filled fields, centi-scaling, strings for numbers, ~150 fields per
activity). We keep that mess in one place:

- **`raw::*`** mirrors the JSON exactly. Field names follow the API (`#[serde(rename_all = "camelCase")]`),
  every field is `Option` or `#[serde(default)]`, and unknown fields are ignored. These types are
  `pub(crate)`.
- **`model::*`** is what the rest of the program sees. Converting `raw → model` goes through
  `units.rs`, which applies the scaling, turns `0` into `None` where 0 means "absent", builds
  timezone-aware datetimes, and parses `YYYYMMDD`.

```rust
pub struct ActivitySummary {
    pub id: LabelId,
    pub name: Option<String>,
    pub sport: SportType,
    pub start: DateTime<FixedOffset>,
    pub distance: Option<Meters>,
    pub elapsed: Duration,
    pub moving: Option<Duration>,
    pub avg_pace: Option<Pace>,          // s/km
    pub avg_hr: Option<u16>,
    pub avg_power: Option<u16>,
    pub ascent: Option<Meters>,
    pub calories_kcal: Option<f64>,
    pub training_load: Option<u32>,
    pub device: Option<String>,
}
```

Newtypes (`Meters(f64)`, `Pace(f64)`, `LabelId(String)`) stop unit mix-ups at compile time.

## 6. Caching

v1 uses a simple file cache; SQLite comes in Phase 5 if needed.

| Data                          | Key                         | TTL / policy                                         |
| ----------------------------- | --------------------------- | ---------------------------------------------------- |
| Activity detail (summary, laps, downsampled series) | `labelId`     | **Forever.** Finished activities don't change. |
| Activity list pages           | query + page                | 10 min. Show the cache first, then refresh in the background. |
| Analyse / day detail          | date range                  | 30 min for ranges that include today. Forever for past-only ranges. |
| Dashboard                     | —                           | 10 min                                               |
| Schedule                      | date range                  | 10 min                                               |
| Workout library               | —                           | 1 hour                                               |
| Exported files (FIT/GPX/...)  | written to the user's chosen dir, not cached | —                                   |

- Location: `directories::ProjectDirs` cache dir (macOS: `~/Library/Caches/<qualifier>.tuicoros/`).
- Format: `serde_json` of the **model** types plus a small header `{ schema_version, fetched_at }`.
  Bumping `schema_version` invalidates old entries.
- Detail series are downsampled to at most ~1,000 points per metric before caching.
- `tuicoros cache clear` and a Settings action wipe the cache.

## 7. Configuration, secrets and logs

See [SECURITY_AND_CONFIG.md](SECURITY_AND_CONFIG.md). In short:

- `config.toml` in the config dir: region, units, theme, page size, default date range, export dir.
- Keychain (via the `keyring` crate): MD5 password hash (only if "remember me") and the session token.
- Logs: `tracing` to a rolling file in the data dir. **Never to stdout** while the TUI is running.
  `RUST_LOG`-style filter through `TUICOROS_LOG`. Tokens and hashes are redacted.

## 8. Terminal safety

- Enter the alternate screen and raw mode once, in `tui.rs`.
- Install a **panic hook** and a `color-eyre` error hook that restore the terminal **before** printing,
  so a crash never leaves the shell broken.
- Handle `SIGTERM` / `Ctrl+C` / `Ctrl+Z` (suspend/resume) cleanly.
- Handle terminal resize. Every screen must render at **80×24** without panicking. Below a minimum
  size, show a "terminal too small" message.

## 9. CLI surface

```
tuicoros                    # launch the TUI (default)
tuicoros login              # interactive login (email, password, region), stores token
tuicoros logout             # remove token + stored credentials
tuicoros status             # who am I, region, token age, cache size
tuicoros api activities [--from YYYY-MM-DD] [--to ...] [--sport run] [--json]
tuicoros api activity <labelId> [--json]
tuicoros api analyse [--from ...] [--to ...] [--json]
tuicoros export <labelId> --format fit|gpx|tcx|kml|csv [-o path]
tuicoros cache clear
```

The `api` subcommands exist so Phase 1 can be verified against a live account **before** any UI is
built. They also make bug reports easy ("run `tuicoros api activity X --json`").
