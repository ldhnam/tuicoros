# CLAUDE.md

Terminal UI for COROS Training Hub in Rust (ratatui + tokio). Phases 0–3 are done (API client, CLI, TUI with Dashboard, Activities + detail with charts/map, Analysis). Next: Phase 4 (Calendar, Workouts). The docs in
`docs/` are the source of truth; read the relevant one before changing code.

- Roadmap and the current phase: `docs/PLAN.md` (tick checkboxes as work lands).
- Private API facts (endpoints, units, error codes): `docs/COROS_API.md`. Record every new finding there.
- Layout and runtime model: `docs/ARCHITECTURE.md`. Decisions: `docs/DECISIONS.md` (add an ADR, don't rewrite).

## Rules

- `crates/coros-api` must not depend on ratatui/crossterm.
- All unit conversion (centi-scaling, quarter-hour time zones, YYYYMMDD, GPS ×1e7, s/km "speed")
  happens in `coros-api/src/units.rs`. UI code only sees `model::*` types.
- `App::update` is synchronous and does no I/O. Network work goes through `DataService` as `Command`s.
- Never log tokens, password hashes, request bodies of `account/login`, or full download URLs.
- Never auto-retry a write request. Writes need a confirm dialog.
- No `unwrap()`/`expect()` outside tests and startup.
- Tests must not hit the network (use wiremock + fixtures; `synthetic_*` fixtures are hand-written, `real*` ones are scrubbed captures). `captures/` (raw, unscrubbed) is git-ignored: never commit it. Live tests are `#[ignore]` and need `COROS_LIVE=1`.
- Commit only scrubbed fixtures (`cargo run -p xtask -- scrub`).
- The owner lives in Vietnam but the account is on the **`us`** server (verified). They mainly **run**: polish running views first.
- Every screen must render at 80×24 and must not panic at any size (see `never_panics_at_any_size`).
- **Don't make the owner's Keychain prompt**: debug builds use the file store; never run a release build or `TUICOROS_SECRETS=keychain` without asking.
- New keys go in `keymap.rs` too (`documented_keys_are_bound` checks them).

## Commands

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo insta review                       # review snapshot changes (or INSTA_UPDATE=always, then inspect the .snap diff)
cargo run -p tuicoros                    # run the TUI
cargo run -p tuicoros -- status
cargo run -p tuicoros -- api activities --raw   # untouched API JSON (scrub before committing)
COROS_LIVE=1 COROS_EMAIL=… COROS_PASSWORD=… COROS_REGION=asia cargo test -p coros-api --test live -- --ignored
cargo +1.88.0 check --workspace --all-targets   # MSRV
```
