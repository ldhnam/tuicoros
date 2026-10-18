# CLAUDE.md

Terminal UI for COROS Training Hub in Rust (ratatui + tokio). Phase 0 (skeleton) is done; next is Phase 1 (API client). The docs in
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
- Tests must not hit the network. Live tests are `#[ignore]` and need `COROS_LIVE=1`.
- Commit only scrubbed fixtures (`cargo run -p xtask -- scrub`).
- The owner's account is in the **Asia** region and they mainly **run**: verify against `asia` first and polish running views first.
- Every screen must render at 80×24 and must not panic at any size (see `never_panics_at_any_size`).

## Commands

```bash
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo insta review                       # review snapshot changes (or INSTA_UPDATE=always, then inspect the .snap diff)
cargo run -p tuicoros                    # run the TUI
cargo run -p tuicoros -- status
cargo +1.88.0 check --workspace --all-targets   # MSRV
```
