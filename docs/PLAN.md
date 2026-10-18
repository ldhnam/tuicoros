# Implementation Plan

Phased so that **each phase ends with something usable and tested**, and so the risky part (the
private API) is proven before UI work starts.

Read first: [ARCHITECTURE.md](ARCHITECTURE.md) · [COROS_API.md](COROS_API.md) ·
[UI_DESIGN.md](UI_DESIGN.md) · [TESTING.md](TESTING.md) · [DECISIONS.md](DECISIONS.md)

## Overview

| Phase | Goal                                   | Ships                                                   | Size |
| ----- | -------------------------------------- | ------------------------------------------------------- | ---- |
| 0 ✅  | Project skeleton                       | Workspace, CI, logging, config, terminal safety         | S    |
| 1 ✅  | **API client, proven live**            | `coros-api` crate + `tuicoros api …` debug CLI + fixtures | L  |
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

## Phase 1 — API client, proven live ✅ (done 2026-10-18)

The riskiest phase. **No UI work** until this phase's exit criteria are met.

**Tasks**

- [x] `Region` and base URLs (done in Phase 0).
- [x] Base URL override: `ClientConfig::base_url` (tests) and `TUICOROS_API_BASE_URL` (dev only).
- [x] **Region auto-detect** at login: tries the configured region, then `asia`, `us`, `eu`. A
      region only counts if `account/query` works with the new token (COROS can accept a login on
      the wrong region and then `1019` every call). `--region` / `COROS_REGION` = try only that one.
- [x] `envelope.rs`: read text → JSON → `result` check → `data`; no-`data`, HTML/429, non-2xx.
- [x] `CorosError` (Api, TokenInvalid, RateLimited, Http, Network, Unexpected, NotAuthenticated, InvalidInput).
- [x] `auth.rs`: MD5, `login()`, `Session`, `userId` as string or number.
- [x] `client.rs`: headers (`accesstoken`, `yfheader`, UA), semaphore (2), timeouts, backoff with
      jitter for 429/5xx/timeouts, one re-login + retry on `1019` for reads only (writes never
      retried), concurrent re-logins collapsed into one.
- [x] `units.rs` + `sport.rs` (61 sports + `Unknown(u32)`).
- [x] Endpoints. Each module keeps private `Raw*` types next to its public models (ADR-005):
  - [x] `account/query`
  - [x] `activity/query` + `all_activities()` (stops on empty page / total pages / limit)
  - [x] `activity/detail/query` (summary, lap groups, samples, feel). HR zones are not modelled yet.
  - [x] `activity/detail/download` → URL → bytes (token never sent to the file host)
  - [x] `analyse/query`, `analyse/dayDetail/query` (84-day chunks, merged and de-duplicated)
  - [x] `dashboard/query` (HRV part)
  - [x] `training/schedule/query` (entries joined to workouts via `idInPlan`)
  - [x] `training/program/query` (steps: time / distance / open / other)
- [x] Secrets in `tuicoros`: keychain (`keyring` v4) with `0600` file fallback; `login`, `logout`, `status`.
- [x] `tuicoros api account|activities|activity|download|analyse|daily|dashboard|schedule|workouts`
      with human output, `--json` (converted) and `--raw` (untouched API data).
- [x] `xtask scrub` (stable fake ids, GPS moved to a fake origin, URLs/tokens/notes removed, series thinned).
- [x] Unit, fixture and wiremock tests: 129 tests, all offline. Plus a read-only live smoke test (`--ignored`).
- [x] End-to-end run of the real binary against a local mock server (login, every `api`
      command, auto re-login, logout; headers and log contents checked).
- [x] **Real fixtures** captured from the owner's account and scrubbed (11 files: activity list;
      12 km run, interval run, trail run, treadmill run, strength; EvoLab, daily, dashboard,
      calendar, workouts) with `tests/real_fixtures.rs` pinning the verified facts.
- [x] **Verified against the live API**; COROS_API.md corrected (§9). Findings: the account is on
      `us`; detail responses are gzip-compressed; `t7d`/`t28d` are load totals (community notes were
      wrong); lap groups, HR zones, sample fields, workout-step laps (no distance) all mapped.
      Live fixes: gzip support, interval-lap distances, zone 0 display, scrubber (place names,
      device ids, avatar).

**Exit criteria**

- [x] On a real account: `tuicoros login` works (region found automatically); `tuicoros api
      activities` lists the last 90 days with correct km, pace and local start time (owner checked
      the list; interval reps match the planned workout).
- [x] `tuicoros api activity <id>` shows correct laps for runs (no rides in the account).
- [ ] Signing in on t.coros.com, then running a command, re-logs in transparently. Proven against
      the mock server; **watch for it in normal use** (`tuicoros status` shows the token age).
- [x] All tests green with no network (locally; CI is switched off for now).

---

## Phase 2 — MVP TUI ✅ (done 2026-10-18)

**Tasks**

- [x] `Action` / `Command` / `DataService` / `RequestId` plumbing (ARCHITECTURE §3). Per-screen
      state instead of a generic `Remote<T>` (the list needs pages, the detail a single value).
- [x] File cache v1: list pages (fresh 10 min, then shown + refreshed), details forever (samples
      thinned to 1000), per account, `0600`, schema-versioned. `tuicoros cache clear`, `logout --purge`.
- [x] Login screen: email, masked password (hashed + wiped on submit), region (auto/US/Asia/EU),
      remember-me (opt-in), spinner, errors, "session expired" notice, email pre-filled.
- [x] Activities screen: table (columns adapt to width), infinite scroll, sport/date filter popup,
      `/` search, `s` sort, sport-aware pace, cached/updated age, empty/error/loading states.
- [x] Activity detail: header, Summary tab (stats, HR zones, effort, note), Laps tab (lap groups
      cycled with `c`, workout steps first, fastest lap `*`), scrolling.
- [x] Footer key hints + `?` help overlay generated from `keymap.rs` (a test checks every documented key is bound).
- [x] Error toasts (auto-hide) + status bar.
- [x] App state tests + 19 render snapshots (login, list states, search, filter, help, detail, sizes 60–100 wide).
- [x] Keychain prompts: one secret read once per run; development builds use the private file.

**Exit criteria**

- [x] Cold start (no cache) shows the first page within ~2 s (live: the list request takes ~0.9 s;
      the whole Dashboard, 4 requests in parallel, 2.4 s). A warm start shows cached data at once.
- [ ] Scrolling 500+ activities stays smooth. *Not measurable on the owner's account (57
      activities); the UI never freezes while loading (all I/O is off the UI task).*
- [x] Works at 80×24 (snapshots at 60×15, 80×24, 100×30; no panic at any size).


---

## Phase 3 — Insight screens ✅ (done 2026-10-18 · one spot-check open)

- [x] Dashboard (tab 1, shown after sign-in): tiles for recovery, fatigue, load ratio (with zone
      meters), resting HR, HRV (or "N more nights"), VO2max, threshold; 28-day acute/chronic load
      chart; this week plan vs actual; recent 5 activities (Enter opens); ability scores.
- [x] Analysis (tab 3): 4/12/24-week window (`w`), moved with `[` `]`; acute vs chronic load chart,
      resting HR, weekly load vs COROS's recommended range, weekly distance (from activities),
      fitness changes (VO2max, threshold HR/pace, stamina, CTI). HRV waits for real HRV data.
- [x] Activity detail **Charts**: HR / pace (inverted, stops capped) / elevation / cadence / power
      (`m`), distance or time axis (`x`), LTTB downsampling to chart width.
- [x] Activity detail **Map**: braille track, aspect-correct projection, start/finish (or `S/F` on
      loops), north up.
- [x] Activity detail **Notes**: effort scale, note, weather, times, device.
- [x] HR zone bars (Summary tab, since Phase 2).
- [x] Snapshots for all of the above (Dashboard, Analysis, Charts, Map, Notes at 80×24 and 100×30).
- [x] Generic cached `Fetch` command + `Slot<T>`; new data: dashboard, daily metrics + weekly loads,
      schedule, activities-in-range. Weather (×10), per-second pace and weekly loads added to `coros-api`.

**Exit criteria**

- [x] Works on the owner's account: Dashboard 2.4 s cold, Analysis 0.4 s, Charts/Map/Notes on a
      real 12 km run; 8 requests for a full tour, no errors.
- [ ] Numbers match t.coros.com (spot-check 5 values). Open question: resting HR differs between
      sources (dashboard 54 vs EvoLab daily 70 on the same day). Which one t.coros.com shows decides
      what the Analysis panel uses.

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
