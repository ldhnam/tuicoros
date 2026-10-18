# tuicoros

A terminal UI for **COROS Training Hub** (<https://t.coros.com>), written in Rust.

See your training without leaving the terminal: today's recovery and load, every activity with
laps, heart-rate zones, pace/HR/elevation charts and a GPS map, and EvoLab trends over 4–24 weeks.

> **Unofficial.** Not affiliated with or endorsed by COROS. The Training Hub API is private and
> undocumented, so this tool may break at any time. Use it for your own account only.

## Status

**v0.3.0**: Phases 0–3 of the [plan](docs/PLAN.md) are done and verified on a real account.

| Screen | What you get |
| --- | --- |
| **1 Dashboard** | Recovery, fatigue, load ratio, resting HR, HRV, VO2max, threshold; 28-day load chart; this week vs plan; recent activities; running abilities |
| **2 Activities** | Every activity (infinite scroll), `/` search, `s` sort, `f` sport/date filter |
| ↳ **Activity detail** | Summary + HR zones · Laps (workout steps, 1 km, 5/10 km) · Charts (HR, pace, elevation, cadence, power) · Map (braille GPS route) · Notes (effort, note, weather) |
| **3 Analysis** | Acute vs chronic load, resting HR, weekly load vs recommended range, weekly distance, fitness changes; 4/12/24-week windows |
| 4 Calendar, 5 Workouts | Coming in Phase 4 |

Data is cached on disk, so the second start is instant. It works at 80×24 and with `NO_COLOR`.

## Run it

You need Rust (stable, 1.88 or newer).

```bash
git clone https://github.com/ldhnam/tuicoros && cd tuicoros
cargo run -p tuicoros            # opens the TUI; sign in on the first screen
```

Press `?` anywhere for the keys. Common ones: `1`–`5` tabs, `↑↓`/`jk` move, `Enter` open, `Esc` back,
`r` refresh, `q` quit. In an activity: `Tab` switches Summary/Laps/Charts/Map/Notes, `m`/`x` change
the chart, `c` the lap grouping. In Analysis: `[` `]` move the window, `w` changes its length.

Your password is hashed (MD5, which is what COROS expects) and never stored. The session token is
kept in the macOS Keychain for installed builds, or in an owner-only file for development builds
(see [security notes](docs/SECURITY_AND_CONFIG.md)). Your COROS region (US / EU / Asia) is detected
automatically.

### Command line

```bash
tuicoros login | logout [--purge] | status
tuicoros api activities [--from 2026-09-01] [--sport run] [--all] [--json|--raw]
tuicoros api activity <ID> [--sport trailRun]
tuicoros api daily --from -28 · api dashboard · api schedule · api workouts
tuicoros api download <ID> --format gpx
tuicoros cache clear
```

(Use `cargo run -p tuicoros -- <command>` until you install it with `cargo install --path crates/tuicoros`.)

## Documentation

| Doc | What's in it |
| --- | ------------ |
| [docs/PLAN.md](docs/PLAN.md) | Roadmap, what's done, what's next |
| [docs/COROS_API.md](docs/COROS_API.md) | The private Training Hub API as verified on a live account: auth, endpoints, units |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Crates, runtime model, API client, caching |
| [docs/UI_DESIGN.md](docs/UI_DESIGN.md) | Screens, keys, formatting rules |
| [docs/SECURITY_AND_CONFIG.md](docs/SECURITY_AND_CONFIG.md) | Credentials, config file, directories, logs |
| [docs/TESTING.md](docs/TESTING.md) | Fixtures, mocks, snapshot tests, live tests |
| [docs/DECISIONS.md](docs/DECISIONS.md) | Architecture decision records |

## Development

```bash
cargo test --workspace          # ~210 tests, no network needed
cargo clippy --workspace --all-targets -- -D warnings
cargo insta review              # review UI snapshot changes
```

Built with ratatui, crossterm, tokio, reqwest (rustls), serde, chrono, keyring and tracing.
Tests use wiremock, insta snapshots and scrubbed real API responses.

## License

MIT
