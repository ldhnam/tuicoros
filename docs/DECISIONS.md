# Architecture Decision Records

Short records of the decisions that shape the project. Add a new ADR instead of rewriting an old one;
mark old ones **Superseded** when they change.

---

## ADR-001: Rust + ratatui + crossterm + tokio

**Status:** Accepted · 2026-10-18

**Decision.** Build the TUI with `ratatui` 0.30 on the `crossterm` 0.29 backend, with `tokio` as the
async runtime and `reqwest` 0.13 (rustls) for HTTP.

**Why.** ratatui is the standard Rust TUI library. It has the widgets we need (Table, Tabs, Chart,
Sparkline, BarChart, Canvas for the GPS map, Gauge), a `TestBackend` for snapshot tests, and an
active community. crossterm works on macOS, Linux and Windows. We need async for non-blocking
network calls, and tokio is what reqwest and crossterm's `EventStream` already support.

**Alternatives.** `cursive` (callback style, a weaker charting story); `tui-realm` (more framework
than we need); a blocking client on a thread pool (simpler, but cancellation and timeouts get awkward).

---

## ADR-002: Two crates — `coros-api` library and `tuicoros` binary

**Status:** Accepted · 2026-10-18

**Decision.** A Cargo workspace with `crates/coros-api` (no terminal dependencies) and
`crates/tuicoros` (the app), plus an `xtask` crate for dev tooling (fixture scrubbing).

**Why.** The API layer is where the risk is (private, undocumented, drifting). Isolating it lets us
test it hard with fixtures and wiremock, and verify it live through the CLI before any UI exists.
It also lets the API client be reused later (another frontend, or a published crate).

---

<a id="adr-003"></a>
## ADR-003: Data source — Training Hub web API first, official COROS MCP later

**Status:** Accepted · 2026-10-18 · **Open for review by the project owner**

**Context.** There are two ways to get COROS data:

| | Training Hub web API (`teamapi.coros.com`) | Official COROS MCP (`mcp.coros.com`) |
|---|---|---|
| Official? | No. Private and may break. | Yes, published by COROS. |
| Auth | email + md5(password) → token | OAuth 2.1 + PKCE in the browser, refresh tokens |
| Data | Activities, details, laps, time series, EvoLab, calendar, workouts. Exactly what t.coros.com shows. | All of that plus sleep, stress, SpO2, daily health |
| Response shape | Stable-ish JSON, well mapped by 4+ community clients | MCP tool results built for LLMs. Structure not yet verified. |
| Rate limits | Aggressive, undocumented | Unknown |
| Effort | Low: plain REST + serde | Medium: MCP client over Streamable HTTP + OAuth loopback flow |

**Decision.** Build v1 on the **Training Hub web API**, since it is what t.coros.com uses and its
shapes are well understood. Put the data access behind a `DataSource` trait inside `DataService`, so
an **official MCP backend** can be added in Phase 6 after a spike confirms its results are
machine-readable.

**Consequences.** We store a credential (the md5 hash, in the keychain, opt-in). We must handle API
drift with fixtures and live smoke tests. Sleep data waits for the MCP backend.

**Revisit if:** COROS blocks third-party use of the private API, the MCP results turn out to be
clean JSON, or users strongly prefer OAuth over entering a password.

---

## ADR-004: Unidirectional data flow (Action → update → render)

**Status:** Accepted · 2026-10-18

**Decision.** Terminal events and network results both become `Action`s. `App::update` changes state
synchronously with no I/O and returns `Command`s. `DataService` runs commands on tokio tasks and
reports back as `Action`s. Rendering reads state only.

**Why.** State changes stay testable without a terminal or a network. Ordering problems (stale
responses, cancelled screens) are handled in one place with request ids. This is the pattern
recommended by the ratatui templates (component + action).

---

## ADR-005: Raw API types separate from domain models

**Status:** Accepted · 2026-10-18

**Decision.** `raw::*` mirrors the JSON exactly and is tolerant (`Option` everywhere, unknown fields
ignored). `model::*` holds unit-correct newtypes. Conversion happens once, in `units.rs`.

**Why.** The API mixes scales between endpoints (centi-scaled detail vs plain list), uses
quarter-hour time zones, `YYYYMMDD` integers, numbers sent as strings, and zero for "missing". Without
a hard boundary these bugs leak into every screen.

---

## ADR-006: Credentials — keychain, opt-in, hash only

**Status:** Accepted · 2026-10-18

**Decision.** Never store the plaintext password. Store the token in the OS keychain (`keyring`
crate). Store `md5(password)` there too, **only** if the user picks "Remember me", so we can re-login
when the token is invalidated (which happens whenever the user signs into t.coros.com or the app).
Fall back to `0600` files when there is no keychain.

**Why.** It balances convenience (tokens get invalidated often) against risk. See
[SECURITY_AND_CONFIG.md](SECURITY_AND_CONFIG.md).

---

## ADR-007: Caching — JSON files first, SQLite only if needed

**Status:** Accepted · 2026-10-18

**Decision.** v1 caches serialized model types as JSON files keyed by request. Move to SQLite
(`rusqlite`, bundled) in Phase 5 **only if** we need queries across the cache (e.g. offline search
across all activities, or year-over-year stats).

**Why.** Files are simple, easy to inspect and delete, and enough for "instant startup" plus "detail
cached forever". SQLite adds migrations and a C build dependency.

---

## ADR-008: Date/time with `chrono`

**Status:** Accepted · 2026-10-18

**Decision.** Use `chrono` (`NaiveDate`, `DateTime<FixedOffset>`). Activities carry their own fixed
UTC offset (quarter-hours), so we don't need a full time zone database.

**Alternatives.** `jiff` has a nicer API and better time zone handling, but `chrono` is what most of
the ecosystem (ratatui examples, serde helpers) uses. Revisit if we need DST-aware calendar math in
the user's local zone.

---

## ADR-009: No mobile API, no upload in v1

**Status:** Accepted · 2026-10-18

**Decision.** Don't use the mobile API (`apieu/apius.coros.com`), which is needed for sleep stages.
Don't implement FIT upload.

**Why.** Mobile login depends on an AES key taken from the Android APK. Upload depends on opaque
signatures hardcoded from COROS clients. Both are fragile and go further than acting as the web app
on the user's behalf. Sleep data should come from the official MCP (ADR-003).

---

## ADR-010: Product name stays `tuicoros`

**Status:** Accepted · 2026-10-18

**Decision.** Keep the name `tuicoros` for the crate, the binary, the `TUICOROS_*` env vars and the
config directories.

**Why.** It says exactly what the tool is, it's free on crates.io, and everything already uses it.
`negsplit`, `zone2` and `fartlek` were considered. They avoid the brand, but they don't say what the
tool is.

**Caveat.** The name contains the COROS trademark. That's fine for a personal, clearly unofficial
tool. Before publishing widely (crates.io, Homebrew), re-check this. If COROS objects, rename
(`negsplit` was the runner-up) and keep "a terminal client for COROS Training Hub" as the tagline.
The README must always say the project is unofficial and not affiliated with COROS.
