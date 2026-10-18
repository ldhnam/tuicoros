# Testing Strategy

The API is private and undocumented, so **tests based on real recorded responses** are what keep
us safe. The UI is tested by rendering to an in-memory buffer and taking snapshots.

## 1. Test pyramid

| Layer                       | Where                        | Tools                         | Runs in CI |
| --------------------------- | ---------------------------- | ----------------------------- | ---------- |
| Unit: units, formatting, md5, envelope | `coros-api/src/*`, `tuicoros/src/format.rs` | `#[test]`, `proptest` for conversions | ✅ |
| Deserialization from fixtures | `coros-api/tests/fixtures_*.rs` | `serde_json`, `insta` (YAML snapshots of the **model** output) | ✅ |
| Client behaviour (HTTP)     | `coros-api/tests/client_*.rs` | `wiremock`, `tokio::test`     | ✅ |
| App state machine           | `tuicoros/src/app.rs` tests, `screens/*` | plain `#[test]`: action in → state + commands out | ✅ |
| Rendering                   | `tuicoros/tests/render_*.rs` | `ratatui::backend::TestBackend` + `insta` | ✅ |
| Live smoke tests            | `coros-api/tests/live.rs`    | `#[ignore]`, needs `COROS_LIVE=1` + credentials | ❌ manual only |

## 2. Fixtures

Fixtures are real API responses, scrubbed, stored in `crates/coros-api/tests/fixtures/<endpoint>/<case>.json`.

Capture them with the debug CLI (Phase 1):

```bash
tuicoros api activities --from 2026-09-01 --to 2026-09-30 --raw > /tmp/activities.json
cargo run -p xtask -- scrub /tmp/activities.json > crates/coros-api/tests/fixtures/activity_query/run_month.json
```

The scrubber (`xtask scrub`) must:

- replace `userId`, `email`, `nickname`, `headPic`, `deviceId`, `labelId`s with stable fake values
  (the same input always maps to the same fake, so references between files still match);
- shift GPS coordinates to a fixed fake origin (keep the shape, drop the location);
- drop `imageUrl`, `fileUrl`, voice note URLs;
- shrink `frequencyList` to about 200 points so the files stay small.

**Required fixture cases** (Phase 1 exit criteria):

| Endpoint                   | Cases                                                                 |
| -------------------------- | --------------------------------------------------------------------- |
| `account/login`            | success, wrong password, token in a numeric `userId` form             |
| `activity/query`           | normal page, empty page, last page, mixed sports, unknown sport type  |
| `activity/detail/query`    | outdoor run with GPS, treadmill (no GPS), bike with power, pool swim, strength, activity with note + RPE |
| `activity/detail/download` | success                                                               |
| `analyse/query`            | normal, `modelValidState: false` (new account)                        |
| `analyse/dayDetail/query`  | 12-week range including days with no watch data                       |
| `dashboard/query`          | with HRV, without HRV                                                 |
| `training/schedule/query`  | with active plan, no plan                                             |
| `training/program/query`   | run intervals, bike, strength                                         |
| errors                     | `1019`, `1001`, HTML 429 body, truncated JSON                         |

## 3. Client behaviour tests (wiremock)

Each case starts a `MockServer`, points `CorosClient` at it with a base URL override, and asserts
both the result and the requests the server received.

- Login sends `md5(password)` in lowercase hex, `accountType: 2`, and the right JSON shape.
- Authenticated calls send `accesstoken` and `yfheader` with the right `userId`.
- `activity/detail/query` puts `labelId` and `sportType` in the **query string**, sends POST with `{}`.
- `analyse/query` is sent as GET (POST would `1001`).
- `1019` → exactly **one** re-login → retry succeeds.
- `1019` → re-login → `1019` again → `CorosError::TokenInvalid` (no infinite loop).
- A **write** that gets `1019` is **not** retried automatically.
- HTML body + 429 → `RateLimited`. Backoff retries 3 times for reads, then gives up.
- Non-2xx with a `0000` envelope → `Http` error.
- Pagination stops on an empty page even if `totalPage` is wrong.
- Semaphore: with `max_concurrent = 2`, a third request waits (checked with a delayed mock).
- Timeout → `Network` error, not a hang.

## 4. Units (most important unit tests)

Each conversion in `units.rs` has table tests built from the values documented in
[COROS_API.md §4](COROS_API.md#4-units-and-data-traps):

- `235759` (detail distance) → `2357.59 m`; `691549` → `6915.49 s`.
- `startTimezone = 8` → `UTC+02:00`; `-20` → `UTC-05:00`; `22` → `UTC+05:30`.
- `20261018` → `2026-10-18`; `0` / invalid → `None` (never panic).
- GPS `577528390` → `57.752839`.
- `calorie = 541449` → `541.4 kcal`.
- `avgSpeed = 312` (s/km) → `5:12 /km` and `11.5 km/h`.
- Zero HR / power / cadence → `None`.
- `proptest`: list ↔ detail scaling agree for the same activity, and conversions never panic on any `i64`.

## 5. App and screen tests

`App::update` is pure, so these tests need no terminal and no network:

```rust
let mut app = App::new_for_test(fixtures::account());
let cmds = app.update(Action::SwitchTab(Tab::Activities));
assert_eq!(cmds, vec![Command::FetchActivities { page: 1, req: RequestId(1), .. }]);

app.update(Action::ActivitiesLoaded { req: RequestId(0), .. }); // stale → ignored
assert!(app.activities().is_loading());
```

Cover: navigation stack, tab switching, stale-response dropping, infinite-scroll trigger, filter →
new query, `Remote` state transitions, login error → region hint, quit confirmation.

## 6. Render snapshots

```rust
let backend = TestBackend::new(100, 30);
let mut terminal = Terminal::new(backend)?;
terminal.draw(|f| app.render(f))?;
insta::assert_snapshot!(terminal.backend());
```

- Every screen × {loading, loaded, empty, error} at **100×30** and **80×24**.
- One test renders every screen at **40×12** and only asserts **no panic** plus the "too small" message.
- Snapshots use fixed `now` (inject a `Clock`) and the `NO_COLOR` theme, so they are stable.
- Review snapshot changes with `cargo insta review`.

## 7. Live smoke tests (manual)

```bash
COROS_LIVE=1 COROS_EMAIL=… COROS_PASSWORD=… COROS_REGION=eu \
  cargo test -p coros-api --test live -- --ignored --test-threads=1
```

- **Read-only endpoints only.** Never run writes against a real account in tests.
- Run sequentially with a short pause between calls (rate limits).
- Purpose: detect API drift. Each test decodes into the **model** types and checks basic sanity
  (dates in range, distance ≥ 0, `labelId` not empty).
- Run before each release and whenever a user reports a decode error.

## 8. CI (`.github/workflows/ci.yml`)

On push and PR, for `ubuntu-latest` and `macos-latest`:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check            # licenses + advisories (optional, from Phase 0)
```

Plus an MSRV job that runs `cargo check` with the `rust-version` from `Cargo.toml`.

## 9. Definition of done (per feature)

- [ ] Fixtures for every new endpoint or response variant, scrubbed.
- [ ] Unit tests for new conversions.
- [ ] wiremock test for any new client behaviour.
- [ ] Snapshot tests for every new screen state at 100×30 and 80×24.
- [ ] `clippy -D warnings` clean, no `unwrap()` / `expect()` outside tests and `main` setup.
- [ ] Docs updated: [COROS_API.md](COROS_API.md) for API findings, [UI_DESIGN.md](UI_DESIGN.md) for keys.
