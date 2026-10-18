# tuicoros

A terminal UI for **COROS Training Hub** (<https://t.coros.com>), written in Rust.

Browse your activities, laps, heart-rate and pace charts, GPS tracks, EvoLab training load, HRV,
training calendar and workout library without leaving the terminal.

> **Status: Phase 0 done** (app skeleton: tabs, config, logging, CI). No COROS data yet. That
> arrives in Phase 1. See [docs/PLAN.md](docs/PLAN.md).
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

## Development

```bash
cargo run -p tuicoros             # launch the (empty) TUI
cargo run -p tuicoros -- status   # show config, paths, region
cargo test --workspace            # all tests (no network needed)
```

## Planned usage

```bash
tuicoros            # launch the TUI
tuicoros login      # sign in from the command line
tuicoros api activities --from 2026-09-01 --json   # raw access for scripting and debugging
tuicoros export <activity-id> --format gpx
```
