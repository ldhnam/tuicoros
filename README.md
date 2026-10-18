# tuicoros

A terminal UI for **COROS Training Hub** (<https://t.coros.com>), written in Rust.

Browse your activities, laps, heart-rate and pace charts, GPS tracks, EvoLab training load, HRV,
training calendar and workout library without leaving the terminal.

> **Status: Phase 1** (API client + command line). `tuicoros login` and `tuicoros api …` work;
> the TUI screens are still placeholders (Phase 2). See [docs/PLAN.md](docs/PLAN.md).
>
> **Unofficial.** Not affiliated with or endorsed by COROS. The Training Hub API is private and
> undocumented, so this tool may break at any time. Use it for your own account only.

## Documentation

| Doc | What's in it |
| --- | ------------ |
| [docs/PLAN.md](docs/PLAN.md) | Phased roadmap, tasks, exit criteria, risks, open questions |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crate layout, runtime model, API client, models, caching, CLI |
| [docs/COROS_API.md](docs/COROS_API.md) | Researched reference for the private Training Hub API: auth, endpoints, unit traps |
| [docs/UI_DESIGN.md](docs/UI_DESIGN.md) | Screens (mockups), keybindings, visual style, formatting rules |
| [docs/SECURITY_AND_CONFIG.md](docs/SECURITY_AND_CONFIG.md) | Credential handling, config file, directories, logging |
| [docs/TESTING.md](docs/TESTING.md) | Fixtures, wiremock, snapshot tests, live smoke tests, CI |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Architecture decision records (ADRs) |

## Planned features (v1)

- Sign in with your COROS account (US / EU / CN regions). Token cached; password never stored.
- **Dashboard:** HRV, resting HR, fatigue, load ratio, this week vs plan, recent activities.
- **Activities:** fast, filterable, infinitely scrolling list across all sports.
- **Activity detail:** summary, laps, HR/pace/elevation/power charts, braille GPS map, notes and RPE.
- **Analysis (EvoLab):** acute vs chronic load, RHR and HRV trends, weekly volume, VO2max and threshold.
- **Calendar and workouts:** planned vs completed, structured workout steps.
- **Export:** FIT / GPX / TCX / KML / CSV.
- Works offline from cache, full keyboard control, works at 80×24.

## Tech stack

Rust (stable, edition 2024) · ratatui · crossterm · tokio · reqwest (rustls) · serde · chrono ·
keyring · tracing. Tested with insta snapshots and wiremock.

## Try it (Phase 1)

```bash
cargo run -p tuicoros -- login                    # email + password; finds your region
cargo run -p tuicoros -- status                   # who you are, region, where secrets live
cargo run -p tuicoros -- api activities           # last 90 days
cargo run -p tuicoros -- api activities --sport run --from 2026-01-01 --all
cargo run -p tuicoros -- api activity <ID>        # summary + laps (add --sport for non-runs)
cargo run -p tuicoros -- api daily --from -28     # RHR, HRV, training load per day
cargo run -p tuicoros -- api schedule             # training calendar
cargo run -p tuicoros -- api download <ID> --format gpx
cargo run -p tuicoros -- logout
```

Every `api` command also takes `--json` (converted data) or `--raw` (COROS's own JSON).

## Development

```bash
cargo test --workspace            # all tests (no network needed)
cargo run -p tuicoros             # the TUI (placeholder screens for now)
```

## Planned usage

```bash
tuicoros            # launch the TUI
tuicoros login      # sign in from the command line
tuicoros api activities --from 2026-09-01 --json   # raw access for scripting and debugging
tuicoros export <activity-id> --format gpx
```
