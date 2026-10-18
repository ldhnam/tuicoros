# COROS Training Hub API — Reference Notes

> **Status:** Researched 2026-10-18 from community clients and implemented in `crates/coros-api`
> (Phase 1). Parsing is tested against **synthetic** fixtures built from this document. **Not yet
> verified against a live account.** See [§9 Verification status](#9-verification-status).

COROS does **not** publish a public REST API. The Training Hub web app (<https://t.coros.com>) talks
to a private JSON backend (`teamapi.coros.com` and its regional twins). Several open-source projects
have mapped it, and this document collects what they agree on. The API can change or break at any
time without notice.

COROS **does** publish an official, OAuth-based **MCP server** (`https://mcp.coros.com/mcp`). It is
covered at the end of this document as a possible second backend (see [DECISIONS.md](DECISIONS.md#adr-003)).

---

## 1. Regions and base URLs

| Region key | Base URL                        | Who                                  |
| ---------- | ------------------------------- | ------------------------------------ |
| `us` (alias `en`) | `https://teamapi.coros.com`   | Americas and "rest of world"         |
| `eu`              | `https://teameuapi.coros.com` | Europe                               |
| `asia` (alias `cn`) | `https://teamcnapi.coros.com` | Mainland China; two clients also use it for Asia |

Gotchas:

- **A token from one region does not work on another.**
- **Region is not geography.** Login can *succeed* against the wrong region and then every following
  call fails with `1019` (access token invalid). At least one German account had to use `us`. When
  auth breaks with a fresh token, suspect the region first and show that hint in the UI.
- **Asia is unclear.** `coros-mcp` and `coros-training-mcp` send Asia accounts to `teamcnapi`, but
  the official MCP routes everything except mainland China and Europe to the US cluster. Phase 1
  checks this with a real Asia account (try `asia`, then `us`) and records the answer here.
- The app should store the region with the token and let the user switch it on the login screen.

## 2. Response envelope

Every JSON response is wrapped:

```json
{ "result": "0000", "message": "OK", "apiCode": "8D2DB17A", "data": { ... } }
```

- `result == "0000"` means success. Anything else is an API error. Surface `result` and `message`.
- Error responses may also carry `tlogId` (a server trace ID). Log it.
- Some endpoints (e.g. `activity/delete`) return the envelope **without** `data`.
- Rate limiting and gateway errors can return **non-JSON** (HTML) bodies, sometimes with HTTP 429.
  Read the body as text first, then parse it, so a broken body becomes a clear transport error rather
  than a JSON parse panic.
- An HTTP non-2xx status that still has a `0000` envelope should be treated as a transport error.

### Known result codes

| `result` | Meaning                          | Action                                                    |
| -------- | -------------------------------- | --------------------------------------------------------- |
| `0000`   | Success                          | —                                                         |
| `1019`   | Access token invalid / expired   | Re-login once (if credentials are available), then retry. If it persists right after login, hint "wrong region?" |
| `1001`   | "Service exceptions" (generic)   | Usually a malformed request (wrong method, non-numeric id). Do not retry. |

Others will be discovered in Phase 1. Record each new code here.

## 3. Authentication

### Login

```
POST {base}/account/login
Content-Type: application/json

{ "account": "<email>", "accountType": 2, "pwd": "<lowercase hex MD5 of the password>" }
```

- The password is sent as `md5(password)` in lowercase hex. No salt.
- `data` contains `accessToken` and user fields (`userId`, `nickname`, `email`, `headPic`, ...).
  `userId` may arrive as a string or a number; accept both.

### Authenticated requests

Send these headers on every call after login:

| Header        | Value                                    | Notes                                                       |
| ------------- | ---------------------------------------- | ----------------------------------------------------------- |
| `accesstoken` | the token                                | Header names are case-insensitive in HTTP; one header is enough. Some clients send both `accessToken` and `accesstoken` to be safe. |
| `yfheader`    | `{"userId":"<userId>"}` (JSON string)    | Sent by the web app and two community clients. Include it.  |
| `Content-Type`| `application/json`                       | Also on GETs, matching the web app.                         |
| `Accept`      | `application/json, text/plain, */*`      |                                                             |
| `User-Agent`  | a normal browser UA string               | One client sends a desktop Chrome UA. Keep it configurable. |

Upload (`activity/fit/import`, out of scope for now) uses `AccessToken` as the header name.

### Token lifetime

- Reported lifetime is about **24 hours**, but this is not guaranteed.
- **Logging into t.coros.com or the phone app can invalidate a token held elsewhere.**
- The API rate-limits login aggressively. **Persist the token** and reuse it across runs. Do not log
  in on every start.
- Strategy: on `1019`, re-login **once** using stored credentials, then retry the request **once**.
  Only retry **idempotent** requests automatically. Never auto-retry a write.

## 4. Units and data traps

The API is not consistent about scaling. Convert at the boundary (in `coros-api`), never in UI code.

| Field / source                           | Raw unit                              | Convert to                          |
| ---------------------------------------- | ------------------------------------- | ----------------------------------- |
| `activity/query` `distance`              | metres                                | metres                              |
| `activity/query` `totalTime`, `workoutTime` | seconds                            | `Duration`                          |
| `activity/query` `startTime`, `endTime`  | epoch **seconds**                     | `DateTime`                          |
| `activity/detail/query` `summary.distance`, `totalTime`, `workoutTime` | **centi**-scaled (÷100) | metres / seconds |
| `activity/detail/query` `startTimestamp`, `endTimestamp`, lap timestamps, `frequencyList[].timestamp` | **centi**-scaled epoch seconds (÷100) | `DateTime` |
| `calorie` (list) / `calories` (detail)   | calories, **not** centi-scaled        | kcal = ÷1000                        |
| `startTimezone`, `endTimezone`, `timezone` | **quarter-hours** east of UTC (`8` = UTC+2) | fixed offset = value × 15 min |
| `avgSpeed` (list)                        | **ambiguous**: s/km per one client, km/h ×100 for rides per another (depends on `speedType`) | **ignored**: pace = distance ÷ moving time |
| GPS (`gpsLat`, `gpsLon`, `startGpsLat`, ...) | degrees × 1e7                     | degrees = ÷1e7                      |
| `date`, `happenDay`, `startDay`, `firstDayInWeek` | `YYYYMMDD` as an **integer**   | `NaiveDate`                         |
| `analyse/query` values                   | unscaled (metres, seconds, epoch s)   | —                                   |
| training schedule `planDistance`, `actualDistance` | sent as **strings**         | parse to number                     |
| Workout step `targetValue` with `targetType == 5` (distance) | metres × 100      | metres                              |
| Workout step intensity with `intensityMultiplier == 1000` | value × 1000         | ÷1000                               |
| `sportFeelInfo.feelType`                 | 1..5 (Very Light..Max Effort), `0` = unrated | `Option<PerceivedExertion>`  |
| `sportFeelInfo.sportNote`                | `""` when unset                       | `Option<String>`                    |
| `labelId`, `userId`, `idInPlan`, `happenDay` (schedule) | string **or** number, depending on endpoint | parse both          |
| Numbers in general                       | sometimes JSON strings (`"48000"`)    | parse both                          |

Other traps:

- Every activity carries the same field set, zero-filled. **`0` often means "not applicable"**, not
  zero. Treat zero HR, power, cadence as "missing" in the UI.
- Two resting HR fields: `rhr` (daily aggregate, shown on the web dashboard) and `testRhr` (measured
  value shown in the phone app). They often differ by several bpm. Show `testRhr` by default.
- `performance == -1` means "no estimate".
- `analyse/query` field names use the API's spelling `recomend*`. Keep the serde renames exact.
- Unknown sport type values appear as COROS adds sports. Model `SportType` with an `Unknown(u32)`
  variant and never fail deserialization on it.

## 5. Endpoints

All paths are relative to the regional base URL. **R** = read-only (in scope), **W** = write (later
phases, needs confirmation in the UI).

### Account

| Method | Path                    | Params | Returns                                   | |
| ------ | ----------------------- | ------ | ----------------------------------------- |-|
| POST   | `account/login`         | body above | token + user                          | R |
| GET    | `account/query`         | —      | user profile (`userId`, `nickname`, `email`, `headPic`, `countryCode`, `birthday` YYYYMMDD) | R |
| GET    | `profile/private/query` | —      | Training Hub dashboard layout preferences | R |

### Activities

| Method | Path                         | Params (query string)                                     | Returns |  |
| ------ | ---------------------------- | --------------------------------------------------------- | ------- |--|
| GET    | `activity/query`             | `pageNumber` (≥1), `size` (1..=200), `startDay`, `endDay` (YYYYMMDD), `modeList` (comma-separated sport values, e.g. `100,102`) | `{ count, totalPage, pageNumber, dataList: [ActivityListItem] }` | R |
| POST   | `activity/detail/query`      | `labelId`, `sportType` (in the **query string**); body `{}` | `{ summary, lapList, frequencyList, sportFeelInfo, zoneList, graphList, pauseList, weather, ... }` | R |
| POST   | `activity/detail/download`   | `labelId`, `sportType`, `fileType`; body `{}`              | `{ fileUrl }` — a signed storage URL; fetch it **without** the token | R |
| GET    | `activity/delete`            | `labelId`                                                 | envelope without `data` | W |
| GET    | `activity/fit/getImportSportList` | —                                                    | sport type catalogue | R |

Notes:

- `sportType` on detail and download **must** be the value from the list item. Defaulting to `100`
  (run) is wrong for every other sport.
- Paginate until a page is empty or `pageNumber >= totalPage`, so a bad `totalPage` can't loop forever.
- `activity/delete` returns success even for an id that never existed. A success is **not** proof of
  deletion.
- Detail responses are large. `frequencyList` has one sample per second or so (thousands of points
  for a 2-hour activity). Downsample before charting and before caching.

`fileType` values:

| Format | `fileType` |
| ------ | ---------- |
| CSV    | `0`        |
| GPX    | `1`        |
| KML    | `2`        |
| TCX    | `3`        |
| FIT    | `4`        |

Useful `ActivityListItem` fields: `labelId`, `name`, `sportType`, `date`, `startTime`, `endTime`,
`startTimezone`, `distance`, `totalTime`, `workoutTime`, `avgSpeed` (ambiguous, see §4), `adjustedPace`, `avgHr`,
`avgCadence`, `avgPower`, `np`, `ascent`, `descent`, `calorie`, `trainingLoad`, `device`, `step`.

Useful detail `summary` fields (centi-scaled where noted above): `name`, `sportType`, `distance`,
`totalTime`, `workoutTime`, `startTimestamp`, `endTimestamp`, `timezone`, `calories`, `avgHr`,
`maxHr`, `avgCadence`, `maxCadence`, `avgPace`, `avgSpeed`, `maxSpeed`, `avgPower`, `maxPower`,
`trainingLoad`, `aerobicEffect`, `anaerobicEffect`, `currentVo2Max`, `elevGain`, `minElev`, `maxElev`.

`lapList` is a list of **lap groupings** (auto-lap by distance, manual laps, ...) told apart by
`type`. Each has `lapItemList` with per-lap `distance`, `time`, `avgHr`, `maxHr`, `avgPace`,
`avgCadence`, `avgPower`, `elevGain`, GPS start/end, and more.

### EvoLab / analytics

| Method | Path                       | Params                         | Returns | |
| ------ | -------------------------- | ------------------------------ | ------- |-|
| GET    | `analyse/query`            | `startDay`, `endDay` (YYYYMMDD) — optional; without them the server returns about the last 28 days | `{ dayList, t7dayList, weekList, record, summaryInfo, tlIntensity, sportDataSummary }` | R |
| GET    | `analyse/dayDetail/query`  | `startDay`, `endDay`           | `{ dayList }` — daily metrics over long ranges (up to ~24 weeks; fetch in 12-week chunks) | R |
| GET    | `dashboard/query`          | —                              | dashboard summary; `summaryInfo.sleepHrvData` holds about the last 7 nights of HRV | R |

These endpoints are **GET only**. POST answers `1001`. Note the param names are `startDay`/`endDay`
here but `startDate`/`endDate` on the training schedule.

`AnalyseDay` fields (per day): `happenDay`, `timestamp`, `distance`, `duration`, `rhr`, `testRhr`,
`t7d` (Load Impact), `t28d` (Base Fitness), `ati` (acute), `cti` (chronic), `trainingLoad`,
`trainingLoadRatio`, `trainingLoadRatioState`, `trainingLoadRatioZoneList`, `tiredRateNew`
(fatigue), `tiredRateStateNew`, `tiredRateNewZoneList`, `performance`, `recomendTlMin/Max`,
`avgSleepHrv`, `sleepHrvBase`, `sleepHrvIntervalList`, `vo2max`, `lthr`, `ltsp`, `staminaLevel`,
`staminaLevel7d`. Days with no watch data carry only the required fields.

`sportDataSummary.modelValidState == false` means EvoLab doesn't have enough data yet. Show an
"insufficient data" state rather than zeros.

HRV list items (`dashboard/query` → `data.summaryInfo.sleepHrvData.sleepHrvList[]`): `happenDay`,
`avgSleepHrv`, `sleepHrvBase`, `sleepHrvSd`, `sleepHrvIntervalList`. Today's values sit on
`sleepHrvData` itself.

### Training calendar and workouts

| Method | Path                           | Params / body                                              | Returns | |
| ------ | ------------------------------ | ---------------------------------------------------------- | ------- |-|
| GET    | `training/schedule/query`      | `startDate`, `endDate` (YYYYMMDD), `supportRestExercise=1` | active plan + `programs`, `weekStages` (with `trainSum` plan vs actual), `entities`, `sportDatasInPlan`, `sportDatasNotInPlan` | R |
| GET    | `training/schedule/querysum`   | date range                                                 | planned calendar aggregates | R |
| POST   | `training/program/query`       | body `{}`                                                  | workout library (`data` is a list of programs with `exercises[]`) | R |
| POST   | `training/plan/query`          | body                                                       | training plans | R |
| GET    | `training/exercise/query`      | sport type                                                 | exercise catalogue | R |
| POST   | `training/program/add`         | program JSON                                               | create workout | W |
| POST   | `training/program/calculate`   | program JSON                                               | recomputes distance/time/load | W (no side effects, but complex payload) |
| POST   | `training/program/delete`      | `["id1", ...]`                                             | delete workouts | W |
| POST   | `training/schedule/update`     | versioned payload                                          | add / move workout on the calendar | W |

An account with no plan gets the same envelope with empty `programs` and `daySummaries`.

Calendar entries are in `entities[]`: each has `happenDay` (string **or** number) and `idInPlan`,
which links to `programs[].idInPlan` (again string or number). Week totals are in
`weekStages[].trainSum` (`planDistance`/`actualDistance` are strings).

Workouts use their **own sport numbering**: `1` = run (all run variants), `2` = indoor bike,
`4` = strength, `200` = road bike, `201` = indoor bike (alt). Not the activity `sportType`.

Workout steps (`exercises[]`): `targetType` `2` = time (`targetValue` seconds), `5` = distance
(`targetValue` metres ×100, and `intensityValue`/`intensityValueExtend` ×1000 when
`intensityMultiplier` is `1000`), `1` = open/"until lap" (`targetValue` 0 means no cap). `sets`
repeats the step.
`TrainingProgram` has `id`, `name`, `sportType`, `duration`, `distance`, `trainingLoad`,
`exerciseNum`, `totalSets`, and the structured definition in `essence` / `simple` / `overview`,
whose shape varies by sport. Treat those as `serde_json::Value` until a sport-specific model is needed.

### Out of scope (for now)

- **Upload** (`activity/fit/import`, STS credentials from `faq.coros.com`, S3 PUT). It relies on opaque
  signatures hardcoded from COROS clients and breaks if they rotate.
- **Sleep stages.** These only come from the mobile API (`apieu.coros.com` / `apius.coros.com`), whose
  login uses an AES key pulled from the Android APK. We will not ship that. Sleep data should come
  from the official MCP backend instead.

## 6. Rate limits and politeness

- Limits are not documented, and community clients describe them as **aggressive**.
- Rules for this app:
  - Cap concurrency at **2 requests** in flight (a semaphore in the client).
  - On HTTP 429 or a non-JSON body: exponential backoff with jitter (1s, 2s, 4s, max 3 tries) for
    reads, then show a "rate limited — try again later" status.
  - Cache aggressively (see [ARCHITECTURE.md](ARCHITECTURE.md#6-caching)). Completed activities never
    change, so cache their detail indefinitely.
  - Never refresh on a timer faster than every 5 minutes. Manual refresh (`r`) is fine.
  - No bulk "download everything" at startup. Backfill only on request.

## 7. Official COROS MCP server (alternative backend)

COROS launched an official MCP server: <https://github.com/coroslab/COROS-MCP>.

- Endpoint: `https://mcp.coros.com/mcp` (gateway), or regional `mcpus` / `mcpeu` / `mcpcn`.
- Auth: standard **OAuth 2.1 with PKCE**, dynamic client registration, `authorization_code` +
  `refresh_token` grants, loopback redirect (their helper uses `http://127.0.0.1:43123/callback`).
  No password ever touches our app.
- Transport: MCP Streamable HTTP, **stateless** (no `Mcp-Session-Id` dependency).
- Tools (as of 2026-10): `querySportRecords`, `getActivityDetail`, `queryActivityLapData`,
  `queryCustomActivityLapData`, `downloadActivityFitFiles`, `queryActivityFitFileDownloadUrls`,
  `queryDailyHealthData`, `querySleepData`, `querySleepHrv`, `queryAvgHeartRate`,
  `queryRestingHeartRate`, `queryStressLevel`, `queryHealthCheckTimeSeries`, `queryStressTimeSeries`,
  `queryRecoveryStatus`, `queryFitnessAssessmentOverview`, `queryTrainingLoadAssessment`,
  `queryTrainingSchedule`, `queryTrainingPlanLibrary`, `queryTrainingPlanDetails`,
  `queryWorkoutLibrary`, `queryWorkoutDetails`, `queryDevices`, `queryUserInfo`, plus write tools
  (`createTrainingPlan`, `scheduleWorkout`, `updateScheduledWorkout`, ...).
- **Unknown:** whether tool results are stable structured JSON or LLM-oriented text. Check this in a
  spike (Phase 6) before committing to it.

Pros: officially supported, OAuth (no password storage), includes sleep, stress and health data.
Cons: shaped for LLMs, schemas may drift, adds an MCP client dependency.

## 8. Sources

Community clients (read 2026-10-18):

- TypeScript: <https://github.com/Pinta365/coros> — the most careful notes on units, errors and regions.
- Python: <https://github.com/cygnusb/coros-mcp> — endpoints for workouts, schedule, analytics, HRV.
- Python: <https://github.com/dholliday3/coros-training-mcp> — workout builders, schedule writes.
- Go: <https://github.com/mtzanidakis/coros-query> — headers (`yfheader`), token cache, re-login retry.
- R: rCoros (CRAN) — US and EU support.
- Official: <https://github.com/coroslab/COROS-MCP>.

## 9. Verification status

What `coros-api` assumes, and how sure we are. Update this table when a live capture confirms or
contradicts something.

| Assumption | Source | Status |
| --- | --- | --- |
| Login: `md5(password)`, `accountType: 2` | 4 clients | Tested (mock), **live pending** |
| Headers `accesstoken` + `yfheader` | 2 clients | Tested (mock), **live pending** |
| `1019` = token invalid | 3 clients | Tested (mock), **live pending** |
| Asia accounts use `teamcnapi.coros.com` | 2 clients; official MCP suggests US | **Unknown**: login auto-detect tries `asia`, then `us`, `eu` |
| List units plain, detail ×100 | Pinta365 (measured) | Tested (synthetic), **live pending** |
| Detail `elevGain`, `aerobicEffect`, `anaerobicEffect` scale | none | **Unknown**: shown raw |
| HR / speed / cadence field names in `frequencyList` | none | **Unknown**: kept in `Sample::other` so `api activity` lists them |
| `ltsp` (threshold pace) unit | none | **Unknown**: kept as `lt_pace_raw` |
| `trainSum` distance unit, program `distance` scale | none | **Unknown**: kept raw |
| Lap group `type` values (auto vs manual) | none | **Unknown** |
| `zoneList` (HR zones) shape | none | Not modelled yet (needed in Phase 3) |
| Wrong-password result code | none | Unknown; handled as a generic API error |

How to check: `tuicoros api <command> --raw` shows the untouched response, and
`cargo run -p xtask -- scrub` turns it into a commit-safe fixture (docs/TESTING.md §2).

