# Security and Configuration

## 1. What we handle

| Secret                     | Sensitivity | Where it lives                                    |
| -------------------------- | ----------- | ------------------------------------------------- |
| Plaintext password         | High        | **Only in memory**, during login. Zeroized right after hashing. Never stored. |
| `md5(password)`            | High: it can log in to COROS by itself | OS keychain, **only** if "Remember me" is on. |
| Access token               | Medium (≈24 h lifetime) | OS keychain. Falls back to a `0600` file if no keychain is available (e.g. headless Linux). |
| `userId`, email, region    | Low         | Stored with the token (same place)                |
| Activity data (GPS tracks) | Personal    | Cache dir. Never sent anywhere except COROS.      |

Why store the MD5 hash instead of the password: COROS only ever sees `md5(password)`, so the hash is
all we need to log in again. It doesn't reveal the plaintext password, which people often reuse on
other sites. It is still a login credential for COROS, so it stays in the keychain.

## 2. Rules

1. **No secrets in logs.** Token, hash and password types are wrapped in `secrecy::SecretString`, so
   `Debug` prints `[REDACTED]`. HTTP logging never prints the `accesstoken` or `yfheader` headers,
   or request bodies for `account/login`.
2. **No secrets in error messages or panics.** Body snippets in `Decode` errors go through a scrubber
   that masks `accessToken`, `pwd`, `email` and GPS fields.
3. **TLS only.** `reqwest` with rustls. No option to disable certificate checks.
4. **File permissions.** Directories are created `0700` on Unix; files we write ourselves
   (tokens, cache) are `0600`. Log files are created by `tracing-appender` with the default
   umask (usually `0644`), so they rely on the `0700` log directory for privacy.
5. **Signed download URLs** from `activity/detail/download` are fetched *without* the token header,
   and are never logged in full (they are bearer URLs).
6. **Env var overrides** for scripting and CI: `COROS_EMAIL`, `COROS_PASSWORD`, `COROS_REGION`,
   `COROS_ACCESS_TOKEN` (+ optional `COROS_USER_ID`) to skip login, and `TUICOROS_SECRETS=file` to
   avoid the keychain. Env vars are visible to other processes owned by the same user.
   `TUICOROS_API_BASE_URL` points every request at another server: **development only** (mock
   servers). Never set it to a server you don't control, because the login request would go there.
7. **Writes need confirmation.** Any future endpoint that changes the account (delete, schedule,
   create) shows a confirm dialog naming exactly what will change, and is never retried automatically.
8. **Logout** removes the token and the hash. (A `--purge` option to also delete the cache comes with the cache in Phase 2.)
9. **Fixtures are scrubbed** before they are committed (see [TESTING.md](TESTING.md#2-fixtures)). Real
   GPS tracks reveal where people live.

## 3. Unofficial API and terms

- The Training Hub backend is private. This is a personal-use client that acts like the web app on
  behalf of the signed-in user, for that user's own data.
- Be polite: low concurrency, caching, no background polling (see
  [COROS_API.md §6](COROS_API.md#6-rate-limits-and-politeness)).
- The README must say clearly that the project is **not affiliated with COROS** and may break.
- If COROS asks third-party clients to stop using the private API, switch to the official MCP backend
  ([DECISIONS.md ADR-003](DECISIONS.md#adr-003)).

## 4. Directory layout

Resolved with `directories::ProjectDirs::from("dev", "tuicoros", "tuicoros")`.

| Purpose | macOS                                              | Linux                              |
| ------- | -------------------------------------------------- | ---------------------------------- |
| Config  | `~/Library/Application Support/dev.tuicoros.tuicoros/config.toml` | `~/.config/tuicoros/config.toml` |
| Data    | `~/Library/Application Support/dev.tuicoros.tuicoros/` (logs, `secrets.json` if the file backend is used) | `~/.local/share/tuicoros/` |
| Cache   | `~/Library/Caches/dev.tuicoros.tuicoros/`          | `~/.cache/tuicoros/`               |

`TUICOROS_CONFIG_DIR`, `TUICOROS_DATA_DIR` and `TUICOROS_CACHE_DIR` override these (handy in tests).

## 5. `config.toml`

Every key is optional. The defaults are shown below. Unknown keys produce a warning, not an error.

```toml
# Region of your COROS account: "us" (Americas / rest of world), "eu", "asia" (also accepts "en", "cn").
region = "eu"

[display]
units = "metric"            # "metric" | "imperial"
theme = "dark"              # "dark" | "light"
icons = "text"              # "text" | "nerdfont" | "emoji"
week_starts_on = "monday"   # "monday" | "sunday"
time_zone = "activity"      # "activity" (offset recorded with the activity) | "local"

[activities]
page_size = 50              # 1..=200
default_range_days = 90

[analysis]
default_weeks = 12          # 4 | 12 | 24

[network]
max_concurrent_requests = 2
timeout_secs = 20
user_agent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/145.0.0.0 Safari/537.36"

[export]
directory = "~/Downloads"
default_format = "fit"      # fit | gpx | tcx | kml | csv

[secrets]
backend = "auto"            # "auto" (keychain, else file) | "keychain" | "file"; or TUICOROS_SECRETS

[log]
level = "info"              # also settable with TUICOROS_LOG=debug
```

Precedence (highest wins): CLI flags → env vars → `config.toml` → built-in defaults.

## 6. Logging

- `tracing` + `tracing-appender` daily rolling file `tuicoros.log` in the data dir. Keep 7 files.
- Default level `info`. `debug` logs request method, path, status, duration and `result` code. Never
  headers or bodies. `trace` adds **scrubbed** response bodies, for schema debugging only.
- `tuicoros status` prints the log path.
- Panics are written to the log too, then shown on screen after the terminal is restored.
