# Implementation Plan

Phased so that **each phase ends with something usable and tested**, and so the risky part (the
private API) is proven before UI work starts.

Read first: [ARCHITECTURE.md](ARCHITECTURE.md) · [COROS_API.md](COROS_API.md) ·
[UI_DESIGN.md](UI_DESIGN.md) · [TESTING.md](TESTING.md) · [DECISIONS.md](DECISIONS.md)

## Overview

| Phase | Goal                                   | Ships                                                   | Size |
| ----- | -------------------------------------- | ------------------------------------------------------- | ---- |
| 0     | Project skeleton                       | Workspace, CI, logging, config, terminal safety         | S    |
| 1     | **API client, proven live**            | `coros-api` crate + `tuicoros api …` debug CLI + fixtures | L  |
| 2     | **MVP TUI**                            | Login, Activities list, Activity detail (Summary, Laps) | L    |
| 3     | Insight screens                        | Dashboard, Analysis (EvoLab), detail Charts + Map + Notes | L  |
| 4     | Planning screens                       | Calendar, Workout library (read-only)                   | M    |
| 5     | Polish and offline                     | Export, cache improvements, themes, imperial units, packaging | M |
| 6     | Optional                               | Official MCP backend spike, writes behind confirmations | M–L  |

Sizes: S ≈ 1–2 days, M ≈ 3–5 days, L ≈ 1–2 weeks of focused work.

---

## Phase 0 — Skeleton ✅ (done 2026-10-18)

**Tasks**

- [x] `git init`, `.gitignore`, `LICENSE` (MIT for now — owner can still change it), `rust-toolchain.toml` (stable).
- [x] Workspace `Cargo.toml` with `crates/coros-api`, `crates/tuicoros`, `xtask`; edition 2024;
      `rust-version = "1.88"`; `[workspace.dependencies]`; `[workspace.lints]` (`unsafe_code = "forbid"`,
      `clippy::unwrap_used` / `expect_used = "warn"`, allowed in tests via `clippy.toml`).
- [x] `tuicoros`: `clap` CLI with `tui` (default) and `status`; global `--config`, `--region`.
- [x] `tui.rs`: enter/leave alternate screen, raw mode, panic hook + `color-eyre` hook that restore
      the terminal first; panics are also logged.
- [x] `event.rs`: crossterm `EventStream` + tick + SIGTERM/SIGHUP, merged with `tokio::select!`.
- [x] Empty `App` with tab bar (shrinks for narrow terminals) and footer, `q` / `Ctrl+C` quit,
      `1`–`5` / `Tab` / `←→` / `h l` switch tabs, resize works, "too small" message below 60×15.
- [x] `tracing` to a daily rolling file (7 kept) in a `0700` dir; `TUICOROS_LOG`.
- [x] `config.rs`: `config.toml` + env (`COROS_REGION`, `TUICOROS_LOG`) + CLI, with defaults,
      validation, and warnings (not errors) for unknown keys, shown in the footer.
- [x] CI workflow: fmt, clippy `-D warnings`, test on Linux + macOS, MSRV 1.88 job.
- [x] `coros-api`: `Region` (`us` / `eu` / `asia`, aliases `en` / `cn`) with base URLs.

Deferred: `Ctrl+Z` suspend/resume → Phase 5 (needs a signal helper crate; not worth it yet).

**Exit criteria**

- [x] `cargo run` shows the empty shell; quitting, `Ctrl+C`, SIGTERM and a panic all leave the
      terminal clean (checked in tmux).
- [ ] CI green — all CI steps pass locally (stable 1.91 and 1.88); the workflow runs for real once
      the repo is pushed to GitHub.

**Dependencies** (Phase 0 uses only some; the rest arrive with Phase 1):

```toml
[workspace.dependencies]
ratatui            = "0.30"
crossterm          = { version = "0.29", features = ["event-stream"] }
tokio              = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time", "signal"] }
tokio-util         = "0.7"          # CancellationToken
futures            = "0.3"
reqwest            = { version = "0.13", default-features = false, features = ["json", "rustls"] }  # check feature names for 0.13
serde              = { version = "1", features = ["derive"] }
serde_json         = "1"
thiserror          = "2"
color-eyre         = "0.6"
clap               = { version = "4", features = ["derive", "env"] }
chrono             = { version = "0.4", features = ["serde"] }
md-5               = "0.11"
secrecy            = "0.10"
keyring            = "4"            # API changed in v4; confirm the current usage
directories        = "6"
toml               = "1"
tracing            = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-appender   = "0.2"
url                = "2"

# dev
insta    = "1"
wiremock = "0.6"
proptest = "1"
```

---

## Phase 1 — API client, proven live

The riskiest phase. **No UI work** until this phase's exit criteria are met.

**Tasks**

- [x] `Region` and base URLs (done in Phase 0).
- [ ] Base URL override (for tests).
- [ ] **Region auto-detect** at login: try the chosen region; on `1019` right after a successful
      login, offer to retry the other regions. The owner's account is in **Asia**: two clients map
      Asia to `teamcnapi`, but the official MCP routes "other regions" to the US cluster, so verify
      `asia` first, then `us`.
- [ ] `envelope.rs`: read text → JSON → `result` check → `data`; handle no-`data`, HTML/429, non-2xx.
- [ ] `CorosError` as in ARCHITECTURE §4.
- [ ] `auth.rs`: `md5` hex, `login()`, `Session { token, user_id, region }`, flexible `userId` decode.
- [ ] `client.rs`: headers (`accesstoken`, `yfheader`, UA), semaphore (2), timeouts, backoff for
      429/HTML, `Reauth` hook with one retry for idempotent requests only.
- [ ] `units.rs` + `sport.rs` (all 60 sport values from COROS_API.md, plus `Unknown(u32)`).
- [ ] Endpoints, each with `raw` + `model` + conversion:
  - [ ] `account/query`
  - [ ] `activity/query` (+ `all_activities()` paginator as a `Stream`)
  - [ ] `activity/detail/query` (summary, laps, frequency series, feel info, zones)
  - [ ] `activity/detail/download` → URL → bytes
  - [ ] `analyse/query`, `analyse/dayDetail/query`
  - [ ] `dashboard/query` (HRV part)
  - [ ] `training/schedule/query`
  - [ ] `training/program/query`
- [ ] Token storage in `tuicoros` (keychain + file fallback); `tuicoros login|logout|status`.
- [ ] `tuicoros api activities|activity|analyse|dashboard|schedule|workouts [--json|--raw]`.
- [ ] `xtask scrub` for fixtures.
- [ ] Capture and commit the fixtures listed in TESTING.md §2. **Runs first** (the owner's main
      sport): outdoor run with GPS, treadmill run, interval workout with laps.
- [ ] Unit, fixture and wiremock tests from TESTING.md §3–4.
- [ ] Update COROS_API.md with anything that differed from the research (new error codes, field
      names, param quirks). **Mark it "verified".**

**Exit criteria**

- On a real account: `tuicoros login` works; `tuicoros api activities --json` lists the last 90
  days with correct km, pace and local start time (cross-checked by hand against t.coros.com for
  3 activities of different sports).
- `tuicoros api activity <id>` shows correct laps for a run and a ride.
- Signing in on t.coros.com, then running a command, re-logs in transparently (with remember-me) or
  asks to log in (without).
- All tests green in CI with no network.

---

## Phase 2 — MVP TUI

**Tasks**

- [ ] `Action` / `Command` / `DataService` / `Remote<T>` / `RequestId` plumbing (ARCHITECTURE §3).
- [ ] File cache v1 for activity lists and details (ARCHITECTURE §6).
- [ ] Login screen (fields, region, remember-me, region hint on `1019`).
- [ ] Activities screen: table, infinite scroll, sport/date filter, `/` search, sort, sport-aware pace.
- [ ] Activity detail: header, Summary tab, Laps tab (lap grouping switch, fastest lap highlighted).
- [ ] Footer key hints + `?` help overlay generated from the keymap.
- [ ] Error toasts + status bar; "cached N min ago".
- [ ] `format.rs` (metric only for now) with unit tests.
- [ ] App state tests + render snapshots for the three screens × states × 2 sizes.

**Exit criteria**

- Cold start (no cache) shows the first page within ~2 s on a normal connection. A warm start shows
  cached data immediately and refreshes in the background.
- Scrolling 500+ activities stays smooth. The UI never freezes while loading.
- Works at 80×24.

👉 **Good point to share an early build and get feedback before Phase 3.**

---

## Phase 3 — Insight screens

- [ ] Dashboard (HRV, RHR, fatigue, load ratio, 28-day load sparkline, this week vs plan, recent 5).
- [ ] Analysis screen: ATI vs CTI chart, RHR + HRV sparklines, weekly distance bars, threshold/VO2max
      panel, range navigation (4/12/24 weeks, 12-week chunked `dayDetail` fetches), "insufficient
      data" state.
- [ ] Activity detail **Charts** tab: metric picker (HR/pace/elev/cadence/power), x-axis toggle
      (distance/time), downsampling (e.g. LTTB) to chart width.
- [ ] Activity detail **Map** tab: Canvas braille track, start/finish markers, fit to bounds,
      keep aspect ratio (correct for latitude).
- [ ] Activity detail **Notes** tab: RPE, note, weather.
- [ ] HR zone bar widget.
- [ ] Snapshot tests for all of the above.

**Exit criteria:** numbers on the Dashboard and Analysis match t.coros.com for the same day/range
(spot-check 5 values).

---

## Phase 4 — Planning screens (read-only)

- [ ] Calendar month view + week view, planned (○) vs done (●), weekly plan-vs-actual sidebar.
- [ ] Link planned workouts to the Workout view, and completed ones to Activity detail.
- [ ] Workouts library: list + structured steps renderer (warm-up, repeat blocks, targets with units).
- [ ] Handle "no active plan" cleanly.

**Exit criteria:** a week with a plan looks the same as on t.coros.com (spot-check).

---

## Phase 5 — Polish and offline

- [ ] Export: `e` in the list or detail → format picker → save to the export dir; `tuicoros export`.
- [ ] Imperial units, light theme, `NO_COLOR`, optional Nerd Font icons.
- [ ] Settings screen (edit the main `config.toml` values in-app).
- [ ] Cache: size display, clear action. Decide on SQLite (ADR-007) based on real needs.
- [ ] Optional background backfill of activity details (rate-limited, can be cancelled).
- [ ] Packaging: `cargo install`, GitHub Releases via `cargo-dist`, Homebrew tap.
- [ ] README with screenshots / asciinema.

---

## Phase 6 — Optional extensions

- [ ] **Spike: official COROS MCP backend** (ADR-003). OAuth PKCE loopback flow, Streamable HTTP
      MCP client (evaluate the `rmcp` crate), check that tool results are structured JSON. If good,
      implement `DataSource` for it, plus sleep/health screens.
- [ ] Writes, each behind a confirm dialog and never auto-retried:
  - [ ] Schedule a library workout on a date (`training/schedule/update`).
  - [ ] Delete an activity (verify afterwards via the list, since delete always "succeeds").
- [ ] FIT file parsing (`fitparser`) for offline detail from exported files.

---

## Risks

| Risk                                                  | Likelihood | Impact | Mitigation |
| ----------------------------------------------------- | ---------- | ------ | ---------- |
| COROS changes or blocks the private API               | Medium     | High   | Tolerant `raw` types, fixtures + live smoke tests, `DataSource` trait, MCP backend as plan B |
| Rate limiting / token invalidation annoys users       | High       | Medium | Token persistence, cache-first, semaphore, backoff, clear status messages |
| Unit/scale bugs (centi-scaling, quarter-hour tz)      | High       | Medium | Single `units.rs` boundary, newtypes, table + property tests, manual cross-checks |
| Wrong region confuses users                           | Medium     | Medium | Region picker + explicit `1019` hint after login |
| Large detail payloads slow the UI                     | Medium     | Low    | Decode off the UI task, downsample, cache model not raw |
| Keychain unavailable (headless Linux, SSH)            | Medium     | Low    | `0600` file fallback, `COROS_ACCESS_TOKEN` env |
| Terminal glyph width issues (emoji / Nerd Font)       | Medium     | Low    | Plain-text icons by default |

## Open questions for the project owner

1. **Data source**: is the Training Hub web API OK for v1, with the official MCP as a later
   backend (ADR-003)? Or should we go OAuth/MCP-first?
2. ~~Which region?~~ **Answered: Asia.** Verify `asia` (teamcnapi) first, then `us`.
3. ~~Which sports?~~ **Answered: running.** Run screens (pace, laps, cadence, HR zones) get polish first.
4. **License** and whether the `coros-api` crate should be published to crates.io.
5. Any **write features** (scheduling workouts) wanted earlier than Phase 6?
