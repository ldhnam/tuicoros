# API fixtures

Full response bodies (envelope included), served by `wiremock` in the tests.

- `synthetic_*.json`: **hand-written** from the shapes documented in
  `docs/COROS_API.md`. They test our parsing of the *documented* API.
- Real captures (from `tuicoros api … --raw`, then `cargo run -p xtask -- scrub`)
  go next to them **without** the `synthetic_` prefix. When a real capture
  disagrees with a synthetic one, the real one wins: fix the code and the docs.

Never commit an unscrubbed capture.
