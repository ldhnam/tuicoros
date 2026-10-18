# UI Design

The layout follows the Training Hub (<https://t.coros.com>) navigation so COROS users feel at home:
**Dashboard · Activities · Analysis · Calendar · Workouts**. All mockups target **100×30**. Every
screen must still work at **80×24** (columns drop, charts shrink).

## 1. Global layout

```
┌ tuicoros ─ 1 Dashboard  2 Activities  3 Analysis  4 Calendar  5 Workouts ──── Nam · EU ┐
│                                                                                        │
│                               (active screen content)                                  │
│                                                                                        │
├────────────────────────────────────────────────────────────────────────────────────────┤
│ ⠋ Loading activities…            cached 12 min ago   ↑↓ move  ⏎ open  / filter  ? help │
└────────────────────────────────────────────────────────────────────────────────────────┘
```

- **Header:** tabs (number keys switch), signed-in name and region on the right.
- **Body:** the active screen.
- **Footer / status bar:** left = spinner + current activity or last error; middle = cache freshness;
  right = the 4–5 most useful keys for this screen (generated from the keymap).
- **Overlays:** help (`?`), confirm dialogs, filter input, toasts for errors (auto-hide after 5 s;
  the full error stays in the log).

## 2. Screens

### 2.1 Login

Shown when there is no valid session.

```
                ┌──────────── Sign in to COROS ────────────┐
                │                                          │
                │  Email     [ nam@example.com          ]  │
                │  Password  [ ••••••••••               ]  │
                │  Region    ( ) Americas (•) Asia ( ) Europe │
                │  [x] Remember me (stores hash in Keychain) │
                │                                          │
                │              [ Sign in ]                 │
                │                                          │
                │  ! Token rejected right after login —    │
                │    your account may be in another region.│
                └──────────────────────────────────────────┘
```

- `Tab` / `Shift+Tab` move between fields, `←→` picks a region, `Space` toggles remember-me, `Enter` submits.
- The password is never echoed. It is hashed (MD5) right away and the plaintext is dropped (zeroized).
- The region hint appears when login succeeds but the first call returns `1019`.

### 2.2 Dashboard

```
┌ Today · Sat 18 Oct ──────────────────────────────────────────────────────────────────┐
│ HRV  52 ms  (base 48–60) ▲   RHR  46 bpm   Fatigue  38 Normal   Load ratio 1.05 Optimal│
├ Training load (28 days) ───────────────────────┬ This week ──────────────────────────┤
│  Base Fitness 62 ─── Load Impact 71 ┄┄┄         │  Run    42.3 km   3:51:20   5×     │
│  ▁▂▃▃▄▅▅▆▆▇▇▆▅▆▇██▇▆▅▆▇▇█▇▆▇█                    │  Bike   61.0 km   2:05:12   1×     │
│                                                │  Plan   48.0 km  done 88%  ███████▒ │
├ Recent activities ─────────────────────────────┴─────────────────────────────────────┤
│ Sat 18  🏃 Morning Run         10.02 km   52:11   5:12/km   148 bpm   TL 78            │
│ Thu 16  🏃 Intervals 6×800      9.40 km   48:37   5:10/km   156 bpm   TL 112           │
│ Wed 15  🚴 Commute             24.10 km   58:02   24.9 km/h 131 bpm   TL 41            │
└──────────────────────────────────────────────────────────────────────────────────────┘
```

Data: `dashboard/query` (HRV), `analyse/query` (last 28 days), `training/schedule/query` (this
week's plan vs actual), and `activity/query` page 1 with size 5. `Enter` on a recent activity opens
its detail.

### 2.3 Activities list

```
┌ Activities ── Sport: All ▾  Range: last 90 days  Search: ──────────────── 214 total ┐
│   Date        Sport   Name                     Dist      Time     Pace     HR   TL  │
│ ▶ 2026-10-18  Run     Morning Run             10.02 km   52:11   5:12/km  148   78  │
│   2026-10-16  Run     Intervals 6×800          9.40 km   48:37   5:10/km  156  112  │
│   2026-10-15  Bike    Commute                 24.10 km   58:02  24.9km/h  131   41  │
│   2026-10-14  Strength Gym                       —       45:00      —     112   22  │
│   …                                                                                 │
│                                            loading page 3/11 ⠙                      │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

- Infinite scroll: fetch the next page when the cursor is within 10 rows of the end.
- `/` searches the loaded rows by name (client-side). `f` opens the filter panel (sport multi-select
  → `modeList`; date range → `startDay` / `endDay`, which triggers a server query).
- `s` cycles the sort column (client-side, over loaded rows only, and the UI says so).
- Pace column adapts to sport: min/km for run/walk/hike, km/h for bike, /100 m for swim, — for strength.
- `e` exports the selected activity (format picker).

### 2.4 Activity detail

Tabs inside the screen: **Summary · Laps · Charts · Map · Notes**. `Tab` / `Shift+Tab` or `h`/`l`
switch between them. `Esc` goes back.

```
┌ Morning Run · Run · Sat 18 Oct 2026 06:42 (UTC+7) · COROS PACE 3 ───────────────────┐
│ [Summary]  Laps   Charts   Map   Notes                                             │
├────────────────────────────────────────────────────────────────────────────────────┤
│ Distance   10.02 km      Moving time  52:11      Avg pace   5:12 /km                │
│ Elev gain  84 m          Elapsed      53:40      Best km    4:58 /km                │
│ Avg HR     148 bpm       Max HR       171 bpm    Cadence    176 spm                 │
│ Power      262 W         Calories     612 kcal   Training load 78                   │
│ Aerobic TE 3.4           Anaerobic TE 1.2        VO2max     53                      │
│                                                                                    │
│ HR zones   Z1 ██ 6%  Z2 ████████ 31%  Z3 ██████████ 41%  Z4 ████ 18%  Z5 █ 4%      │
└────────────────────────────────────────────────────────────────────────────────────┘
```

**Laps:** a table from the selected lap grouping (`g` cycles between auto-lap and manual laps). The
fastest lap is highlighted.

**Charts:** ratatui `Chart` with a selectable metric (`m` cycles: HR, pace, elevation, cadence,
power) against distance or time (`x` toggles). Data is downsampled to the chart width.

```
│ Heart rate (bpm) vs distance (km)                                            │
│ 180┤                                     ⣀⡠⠔⠒⠉⠉⠒⠢⣀                              │
│ 160┤               ⢀⣀⠤⠤⠒⠒⠉⠉⠉⠉⠉⠒⠒⠒⠤⠤⠔⠒⠉           ⠉⠒⢄                           │
│ 140┤    ⣀⠤⠔⠒⠉⠉⠉⠉⠉                                    ⠑⠢⣀                        │
│ 120┤⠔⠊⠁                                                                        │
│    └┬──────────┬──────────┬──────────┬──────────┬──────────┬                  │
│     0          2          4          6          8         10                  │
```

**Map:** the GPS track drawn on a ratatui `Canvas` with braille markers, fitted to the bounding box.
Start and finish are marked; lap splits are optional (`p`). No tiles; it's a route outline.

**Notes:** perceived exertion (1–5) and the free-text note from `sportFeelInfo`, plus weather when
present.

### 2.5 Analysis (EvoLab)

Range selector on top: `[` / `]` move the window, `w` switches between 4 / 12 / 24 weeks.

```
┌ Analysis ── 12 weeks: 28 Jul – 19 Oct ─────────────────────────────────────────────┐
│ Training load: ATI (acute) ── vs CTI (chronic) ┄┄          Load ratio  1.05 Optimal │
│ 90┤                                   ⢀⡠⠒⠉⠉⠑⠢⡀    ⣀⠤⠒⠉                               │
│ 60┤┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄                  │
│ 30┤⠒⠒⠢⠤⠤⣀⣀⣀⡠⠤⠤⠒⠒⠉⠉                                                                 │
├ Resting HR ───────────────────────┬ Overnight HRV ─────────────────────────────────┤
│ ▅▅▄▅▄▄▃▄▄▃▃▃▄▃▃▂▃▃▂▂▃▂▂▂  46 bpm   │ ▃▄▅▄▅▆▅▄▅▆▆▅▆▇▆▅▆▇▆▇▆▇  52 ms (base 48–60)     │
├ Weekly distance ──────────────────┴─────────────────────────────────────────────────┤
│ W31 ███████ 32   W32 █████████ 41   W33 ██████████ 46   W34 ████ 18 (recovery) …    │
├ Threshold & fitness ────────────────────────────────────────────────────────────────┤
│ VO2max 53   LTHR 168 bpm   LT pace 4:31/km   Stamina 82%   Running performance 63    │
└────────────────────────────────────────────────────────────────────────────────────┘
```

When `sportDataSummary.modelValidState` is false: "EvoLab needs more data — keep training with
your watch."

### 2.6 Calendar

Month view by default; `v` toggles week view. Each day cell shows planned workouts (outlined) and
completed activities (filled). A sidebar shows the selected week's plan vs actual from
`weekStages[].trainSum`.

```
┌ Calendar ── October 2026 ─────────────────────────────────────┬ Week 42 ─────────────┐
│  Mon      Tue      Wed      Thu      Fri      Sat      Sun     │ Plan    48.0 km  5×  │
│                    1        2        3        4        5       │ Actual  42.3 km  4×  │
│                    ●Run 8k  ○Rest    ●Run 10k                  │ Load    310 / 340    │
│  …                                                             │                      │
│ 13       14       15       16       17      [18]      19       │ Sat 18               │
│ ○Easy 8k ●Gym     ●Bike    ●6×800   ○Rest   ●Run 10k ○Long 18k │ ● Morning Run 10 km  │
└────────────────────────────────────────────────────────────────┴──────────────────────┘
```

`Enter` on a completed activity opens its detail. `Enter` on a planned workout opens the workout view.

### 2.7 Workouts (library)

A list on the left and the structured steps on the right (read-only in v1).

```
┌ Workouts ─────────────────────┬ 6×800 m @ 3:50 ────────────────────────────────────┐
│ ▶ 6×800 m @ 3:50   Run  55:00 │ Warm-up      15:00   Z2                               │
│   Easy 45          Run  45:00 │ Repeat 6×                                             │
│   Sweet spot 3×12  Bike 1:15  │   Run        800 m   3:45–3:55 /km                     │
│   Full body A      Strength   │   Recover    2:00    jog                               │
│                               │ Cool-down    10:00   Z1                                │
│                               │ Est. 9.4 km · 55 min · TL 105                          │
└───────────────────────────────┴───────────────────────────────────────────────────┘
```

### 2.8 Help overlay

`?` opens a centred, scrollable list of every key for the current screen plus the global keys. It
is generated from `keymap.rs`, so it can't drift from the real bindings.

## 3. Keybindings

Arrow keys and vim keys both work everywhere.

| Key                 | Global action                    |
| ------------------- | -------------------------------- |
| `1`–`5`             | Switch tab                       |
| `?`                 | Help                             |
| `q`                 | Back / quit from the top level (asks first if a task is running) |
| `Ctrl+C`            | Quit immediately                 |
| `Esc`               | Close overlay / go back          |
| `r`                 | Refresh the current view (skips the cache) |
| `↑↓` / `j k`        | Move                             |
| `PgUp PgDn` / `Ctrl+u Ctrl+d` | Page                   |
| `g` / `G`           | Top / bottom                     |
| `Enter`             | Open                             |
| `/`                 | Search / filter                  |
| `u`                 | Toggle metric / imperial         |

Screen-specific keys are listed in each section above and in the footer.

## 4. Visual style

- **Colours:** a small palette in `theme.rs` with dark (default) and light variants. Respect
  `NO_COLOR`. Sport colours: run = green, bike = blue, swim = cyan, strength = magenta, other = grey.
- **HR / intensity zones:** five fixed colours (grey → blue → green → yellow → red), always shown
  with labels too, so colour is never the only signal.
- **Icons:** plain-text sport labels by default. Nerd Font / emoji icons are opt-in in config,
  because many terminals render them at the wrong width.
- **Numbers:** right-aligned, with units in the header where the column is uniform.
- **Empty states:** every list has one ("No activities in this range — press f to change filters").
- **Errors:** one short line in the status bar plus a toast. Never a stack trace on screen.

## 5. Formatting rules (`format.rs`)

| Value      | Metric                      | Imperial                     |
| ---------- | --------------------------- | ---------------------------- |
| Distance   | `10.02 km`, `<1 km → 850 m` | `6.23 mi`                    |
| Pace (run) | `5:12 /km`                  | `8:22 /mi`                   |
| Speed (bike)| `24.9 km/h`                | `15.5 mph`                   |
| Swim pace  | `1:52 /100m`                | `1:42 /100yd`                |
| Duration   | `52:11`, `1:05:12`          | same                         |
| Elevation  | `84 m`                      | `276 ft`                     |
| Dates      | `Sat 18 Oct`, ISO in tables | same                         |
| Time zone  | the activity's own offset (from `startTimezone`), not the computer's | same |
