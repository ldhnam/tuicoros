//! `tuicoros api …`: direct API access for debugging, scripting and capturing fixtures.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::PathBuf;

use chrono::{Datelike, Local, NaiveDate};
use color_eyre::Result;
use color_eyre::eyre::WrapErr;
use coros_api::{
    ActivityDetail, ActivityQuery, ActivitySummary, Analyse, ApiRequest, CorosClient, DailyMetrics,
    Dashboard, DateRange, FileFormat, Intensity, LabelId, LapGroup, Schedule, SportCategory,
    SportType, StepKind, StepTarget, Workout, WorkoutStep, requests,
};
use serde::Serialize;

use super::Ctx;
use crate::cli::{ApiCommand, OutputArgs, RangeArgs};
use crate::config::ExportFormat;
use crate::format::{self, MISSING, fit};

pub async fn run(ctx: &Ctx<'_>, command: &ApiCommand) -> Result<()> {
    let (client, _session) = ctx.signed_in_client()?;
    let config = &ctx.loaded.config;
    let today = Local::now().date_naive();

    match command {
        ApiCommand::Account { output } => {
            let request = requests::account();
            if raw(&client, output, &request).await? {
                return Ok(());
            }
            let account = client.account().await?;
            if json(output, &account)? {
                return Ok(());
            }
            println!("User ID   {}", account.user_id);
            println!("Nickname  {}", format::opt(account.nickname.as_deref()));
            println!("Email     {}", format::opt(account.email.as_deref()));
            println!("Country   {}", format::opt(account.country_code.as_deref()));
            println!("Birthday  {}", format::opt(account.birthday));
        }

        ApiCommand::Activities {
            range,
            sports,
            page,
            size,
            all,
            limit,
            output,
        } => {
            let to = range.to.unwrap_or(today);
            let from = range.from.unwrap_or_else(|| {
                DateRange::last_days(to, config.activities.default_range_days).start
            });
            let query = ActivityQuery {
                page: *page,
                size: size.unwrap_or(u32::from(config.activities.page_size)),
                from: Some(from),
                to: Some(to),
                sports: sports.clone(),
            };
            if raw(&client, output, &requests::activities(&query)).await? {
                return Ok(());
            }
            let (items, footer) = if *all {
                let items = client.all_activities(&query, *limit).await?;
                let footer = format!("{} activities from {from} to {to}", items.len());
                (items, footer)
            } else {
                let page = client.activities(&query).await?;
                let footer = format!(
                    "{} activities from {from} to {to} (page {} of {}, {} total; --all for every page)",
                    page.items.len(),
                    page.page,
                    page.total_pages.max(1),
                    page.total_count
                );
                (page.items, footer)
            };
            if json(output, &items)? {
                return Ok(());
            }
            print_activities(&items);
            println!("\n{footer}");
        }

        ApiCommand::Activity { id, sport, output } => {
            let id = LabelId(id.clone());
            if raw(&client, output, &requests::activity_detail(&id, *sport)).await? {
                return Ok(());
            }
            let detail = client.activity_detail(&id, *sport).await?;
            if json(output, &detail)? {
                return Ok(());
            }
            print_detail(&detail, *sport);
        }

        ApiCommand::Download {
            id,
            sport,
            format,
            output,
        } => {
            let format = format.unwrap_or(match config.export.default_format {
                ExportFormat::Fit => FileFormat::Fit,
                ExportFormat::Gpx => FileFormat::Gpx,
                ExportFormat::Tcx => FileFormat::Tcx,
                ExportFormat::Kml => FileFormat::Kml,
                ExportFormat::Csv => FileFormat::Csv,
            });
            let path = match output {
                Some(path) => path.clone(),
                None => expand_home(&config.export.directory)
                    .join(format!("{id}.{}", format.extension())),
            };
            let bytes = client
                .download_activity_file(&LabelId(id.clone()), *sport, format)
                .await?;
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)
                    .wrap_err_with(|| format!("cannot create {}", dir.display()))?;
            }
            std::fs::write(&path, &bytes)
                .wrap_err_with(|| format!("cannot write {}", path.display()))?;
            println!("Saved {} ({} bytes)", path.display(), bytes.len());
        }

        ApiCommand::Analyse { range, output } => {
            let range = optional_range(range, today, 28)?;
            if raw(&client, output, &requests::analyse(range)).await? {
                return Ok(());
            }
            let analyse = client.analyse(range).await?;
            if json(output, &analyse)? {
                return Ok(());
            }
            print_analyse(&analyse);
        }

        ApiCommand::Daily { range, output } => {
            let days = u32::from(config.analysis.default_weeks) * 7;
            let range = required_range(range, today, days)?;
            if output.raw {
                // Raw output is per API call; show the first chunk only.
                let first = range.chunks(84)[0];
                return raw(&client, output, &requests::day_detail(first))
                    .await
                    .map(|_| ());
            }
            let daily = client.day_detail(range).await?;
            if json(output, &daily)? {
                return Ok(());
            }
            print_days(&daily.days);
            for week in &daily.weeks {
                println!(
                    "Week of {}: load {} (recommended {}–{})",
                    week.week_start,
                    format::opt_f(week.training_load, 0),
                    format::opt_f(week.recommended_min, 0),
                    format::opt_f(week.recommended_max, 0)
                );
            }
        }

        ApiCommand::Dashboard { output } => {
            if raw(&client, output, &requests::dashboard()).await? {
                return Ok(());
            }
            let dashboard = client.dashboard().await?;
            if json(output, &dashboard)? {
                return Ok(());
            }
            print_dashboard(&dashboard);
        }

        ApiCommand::Schedule { range, output } => {
            let monday =
                today - chrono::Duration::days(i64::from(today.weekday().num_days_from_monday()));
            let range = match (range.from, range.to) {
                (None, None) => DateRange::new(monday, monday + chrono::Duration::days(27))?,
                _ => required_range(range, today, 28)?,
            };
            if raw(&client, output, &requests::schedule(range)).await? {
                return Ok(());
            }
            let schedule = client.schedule(range).await?;
            if json(output, &schedule)? {
                return Ok(());
            }
            print_schedule(&schedule, range);
        }

        ApiCommand::Workouts { output } => {
            if raw(&client, output, &requests::workouts()).await? {
                return Ok(());
            }
            let workouts = client.workouts().await?;
            if json(output, &workouts)? {
                return Ok(());
            }
            print_workouts(&workouts);
        }
    }
    Ok(())
}

/// `--raw`: print the API's `data` untouched. Returns whether it handled output.
async fn raw(client: &CorosClient, output: &OutputArgs, request: &ApiRequest) -> Result<bool> {
    if !output.raw {
        return Ok(false);
    }
    let data = client.send_raw(request).await?;
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &data)?;
    writeln!(stdout)?;
    Ok(true)
}

/// `--json`: print converted model data. Returns whether it handled output.
fn json<T: Serialize>(output: &OutputArgs, value: &T) -> Result<bool> {
    if !output.json {
        return Ok(false);
    }
    let mut stdout = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, value)?;
    writeln!(stdout)?;
    Ok(true)
}

fn optional_range(
    range: &RangeArgs,
    today: NaiveDate,
    default_days: u32,
) -> Result<Option<DateRange>> {
    if range.from.is_none() && range.to.is_none() {
        return Ok(None);
    }
    required_range(range, today, default_days).map(Some)
}

fn required_range(range: &RangeArgs, today: NaiveDate, default_days: u32) -> Result<DateRange> {
    let to = range.to.unwrap_or(today);
    let from = range
        .from
        .unwrap_or_else(|| DateRange::last_days(to, default_days).start);
    Ok(DateRange::new(from, to)?)
}

fn expand_home(path: &std::path::Path) -> PathBuf {
    match (path.strip_prefix("~"), directories::BaseDirs::new()) {
        (Ok(rest), Some(dirs)) => dirs.home_dir().join(rest),
        _ => path.to_path_buf(),
    }
}

fn print_activities(items: &[ActivitySummary]) {
    println!(
        "{:<20}  {:<10}  {:<13}  {:<22}  {:>9}  {:>8}  {:>10}  {:>4}  {:>4}",
        "ID", "Date", "Sport", "Name", "Distance", "Time", "Pace", "HR", "Load"
    );
    for a in items {
        let date = a.date.or_else(|| a.start.map(|s| s.date_naive()));
        println!(
            "{:<20}  {:<10}  {:<13}  {:<22}  {:>9}  {:>8}  {:>10}  {:>4}  {:>4}",
            a.id,
            format::opt(date),
            fit(&sport_name(a.sport), 13),
            fit(a.name.as_deref().unwrap_or(""), 22),
            format::distance(a.distance),
            format::duration(a.moving.or(a.elapsed)),
            format::pace(a.avg_pace, a.sport.category()),
            format::opt(a.avg_hr),
            format::opt(a.training_load),
        );
    }
}

fn sport_name(sport: SportType) -> String {
    match sport {
        SportType::Unknown(code) => format!("Other ({code})"),
        known => known.label().to_owned(),
    }
}

fn print_detail(d: &ActivityDetail, requested: SportType) {
    let s = &d.summary;
    let sport = s.sport.unwrap_or(requested);
    let cat = sport.category();
    if s.sport.is_some_and(|actual| actual != requested) {
        println!(
            "note: this is a {} activity; pass --sport {} for correct data\n",
            sport.label(),
            sport.key()
        );
    }
    println!(
        "{} · {}",
        s.name.as_deref().unwrap_or("(no name)"),
        sport_name(sport)
    );
    if let Some(start) = s.start {
        println!(
            "Start        {}",
            start.format("%a %d %b %Y %H:%M (UTC%:z)")
        );
    }
    println!("Distance     {}", format::distance(s.distance));
    println!(
        "Moving time  {}   Elapsed {}",
        format::duration(s.moving),
        format::duration(s.elapsed)
    );
    if s.avg_pace.is_some() {
        let mut line = format!("Avg pace     {}", format::pace(s.avg_pace, cat));
        if s.adjusted_pace.is_some() {
            line += &format!("   Grade-adjusted {}", format::pace(s.adjusted_pace, cat));
        }
        if s.best_km.is_some() {
            line += &format!("   Best km {}", format::pace(s.best_km, cat));
        }
        println!("{line}");
    }
    let hr_source = match d.heart_rate_sensor() {
        Some(sensor) => format!("   from {} (HR monitor)", sensor.model_name()),
        None if s.avg_hr.is_some() => "   from the wrist".to_owned(),
        None => String::new(),
    };
    println!(
        "Heart rate   avg {}  max {}{hr_source}",
        format::opt(s.avg_hr),
        format::opt(s.max_hr)
    );
    if s.avg_cadence.is_some() {
        println!(
            "Cadence      avg {}  max {}   Stride {} cm",
            format::opt(s.avg_cadence),
            format::opt(s.max_cadence),
            format::opt(s.avg_stride_length_cm)
        );
    }
    if s.avg_power.is_some() {
        println!(
            "Power        avg {} W  max {} W",
            format::opt(s.avg_power),
            format::opt(s.max_power)
        );
    }
    if s.elevation_gain.is_some() || s.elevation_loss.is_some() {
        let range = match (s.min_elevation, s.max_elevation) {
            (Some(lo), Some(hi)) => format!("   range {lo:.0}–{hi:.0} m"),
            _ => String::new(),
        };
        println!(
            "Elevation    +{} m / -{} m{range}",
            format::opt_f(s.elevation_gain.map(|m| m.0), 0),
            format::opt_f(s.elevation_loss.map(|m| m.0), 0),
        );
    }
    println!(
        "Load {}   Training effect {} aerobic / {} anaerobic   Calories {} kcal",
        format::opt(s.training_load),
        format::opt_f(s.aerobic_effect, 1),
        format::opt_f(s.anaerobic_effect, 1),
        format::opt_f(s.calories_kcal, 0),
    );
    if let Some(rpe) = d.feel.perceived_exertion {
        println!("Effort       {}", rpe.label());
    }
    if let Some(note) = &d.feel.note {
        println!("Note         {note}");
    }

    if !d.hr_zones.is_empty() {
        println!("\nHeart-rate zones");
        for z in d
            .hr_zones
            .iter()
            .filter(|z| z.time.is_some_and(|t| t.as_secs() > 0))
        {
            let bar = "█".repeat((z.percent.unwrap_or(0.0) / 4.0).round() as usize);
            // Zone 0 is "below zone 1"; COROS sends it with zone 1's bounds.
            let bounds = if z.zone == 0 {
                format!("<{}", format::opt(z.low_bpm))
            } else {
                format!("{}–{}", format::opt(z.low_bpm), format::opt(z.high_bpm))
            };
            println!(
                "  Z{} {:<8} {:>7}  {:>3}% {bar}",
                z.zone,
                bounds,
                format::duration(z.time),
                format::opt_f(z.percent, 0)
            );
        }
    }

    // Show the most useful lap group in full; list the others.
    if let Some(group) = LapGroup::preferred(&d.lap_groups) {
        println!("\n{}", group.label());
        println!(
            "  {:>3}  {:<9}  {:>9}  {:>8}  {:>10}  {:>4}  {:>4}  {:>4}",
            "#", "Step", "Distance", "Time", "Pace", "HR", "Cad", "Pwr"
        );
        for (j, lap) in group.laps.iter().enumerate() {
            let fastest = if group.fastest.contains(&(j as u32)) {
                " *"
            } else {
                ""
            };
            println!(
                "  {:>3}  {:<9}  {:>9}  {:>8}  {:>10}  {:>4}  {:>4}  {:>4}{fastest}",
                lap.index,
                lap.step_kind.map_or("", |k| k.label()),
                format::distance(lap.distance),
                format::duration(lap.duration),
                format::pace(lap.pace, cat),
                format::opt(lap.avg_hr),
                format::opt(lap.avg_cadence),
                format::opt(lap.avg_power),
            );
        }
        let others: Vec<String> = d
            .lap_groups
            .iter()
            .filter(|g| !std::ptr::eq(*g, group) && !g.laps.is_empty() && g.kind != Some(-1))
            .map(|g| format!("{} ({})", g.label(), g.laps.len()))
            .collect();
        if !others.is_empty() {
            println!("  also: {} (see --json)", others.join(", "));
        }
    }

    let gps = d.samples.iter().filter(|s| s.lat.is_some()).count();
    let hr = d.samples.iter().filter(|s| s.heart_rate.is_some()).count();
    let fields: BTreeSet<&str> = d
        .samples
        .iter()
        .flat_map(|s| s.other.keys().map(String::as_str))
        .collect();
    println!(
        "\nSamples      {} ({} with GPS, {} with heart rate){}",
        d.samples.len(),
        gps,
        hr,
        if fields.is_empty() {
            String::new()
        } else {
            format!(
                "; also: {}",
                fields.into_iter().collect::<Vec<_>>().join(", ")
            )
        }
    );
}

fn print_analyse(a: &Analyse) {
    if a.model_valid == Some(false) {
        println!("EvoLab doesn't have enough data yet; values below may be empty.\n");
    }
    print_days(&a.days);
    if let Some(latest) = a.rolling_7d.last().or(a.days.last()) {
        println!(
            "\nLatest ({}): VO2max {}  threshold HR {}  threshold pace {}  stamina {}",
            latest.date,
            format::opt_f(latest.vo2max, 0),
            format::opt(latest.lthr),
            format::pace(latest.lt_pace, SportCategory::Run),
            format::opt_f(latest.stamina_level, 0)
        );
    }
    for week in &a.weeks {
        println!(
            "Week of {}: load {} (recommended {}–{})",
            week.week_start,
            format::opt_f(week.training_load, 0),
            format::opt_f(week.recommended_min, 0),
            format::opt_f(week.recommended_max, 0)
        );
    }
}

fn print_days(days: &[DailyMetrics]) {
    println!(
        "{:<10}  {:>4}  {:>4}  {:>7}  {:>5}  {:>5}  {:>5}  {:>7}  {:>6}",
        "Date", "RHR", "Load", "7d load", "ATI", "CTI", "Ratio", "Fatigue", "VO2max"
    );
    for d in days {
        println!(
            "{:<10}  {:>4}  {:>4}  {:>7}  {:>5}  {:>5}  {:>5}  {:>7}  {:>6}",
            d.date,
            format::opt(d.test_rhr.or(d.rhr)),
            format::opt(d.training_load),
            format::opt_f(d.load_7d, 0),
            format::opt_f(d.acute_load, 0),
            format::opt_f(d.chronic_load, 0),
            format::opt_f(d.load_ratio, 2),
            format::opt_f(d.fatigue, 0),
            format::opt_f(d.vo2max, 0),
        );
    }
}

fn print_dashboard(d: &Dashboard) {
    println!(
        "Recovery     {}%  (fully recovered in {} h)",
        format::opt_f(d.recovery_pct, 0),
        format::opt_f(d.full_recovery_hours, 0)
    );
    println!(
        "Resting HR   {}   Max HR {}",
        format::opt(d.resting_hr),
        format::opt(d.max_hr)
    );
    println!(
        "Threshold    {} bpm  {}",
        format::opt(d.lthr),
        format::pace(d.lt_pace, SportCategory::Run)
    );
    println!("Stamina      {}", format::opt_f(d.stamina_level, 1));
    let s = &d.scores;
    println!(
        "Abilities    aerobic endurance {}  threshold {}  anaerobic endurance {}  anaerobic capacity {}",
        format::opt_f(s.aerobic_endurance, 1),
        format::opt_f(s.lactate_threshold, 1),
        format::opt_f(s.anaerobic_endurance, 1),
        format::opt_f(s.anaerobic_capacity, 1)
    );
    match &d.hrv {
        None => println!("HRV          no data"),
        Some(hrv) if hrv.nights.is_empty() && hrv.latest.is_none() => match hrv.remaining_wear_days
        {
            Some(n) => {
                println!("HRV          not ready: wear the watch overnight {n} more night(s)")
            }
            None => println!("HRV          no recent nights"),
        },
        Some(hrv) => {
            println!(
                "HRV          baseline {} ms",
                format::opt_f(hrv.baseline, 0)
            );
            for night in hrv.nights.iter().chain(hrv.latest.iter()) {
                println!(
                    "  {}  {:>3.0} ms  (sd {})",
                    night.date,
                    night.avg,
                    format::opt_f(night.std_dev, 0)
                );
            }
        }
    }
}

fn print_schedule(s: &Schedule, range: DateRange) {
    match &s.plan_name {
        Some(name) => println!(
            "Plan: {name} ({} – {})\n",
            format::opt(s.plan_start),
            format::opt(s.plan_end)
        ),
        None => println!("No active training plan.\n"),
    }
    let entries: Vec<_> = s
        .entries
        .iter()
        .filter(|e| e.date >= range.start && e.date <= range.end)
        .collect();
    if entries.is_empty() {
        println!("Nothing scheduled from {} to {}.", range.start, range.end);
    }
    for e in entries {
        let (name, time, dist) = match &e.workout {
            Some(w) => (
                w.name.clone().unwrap_or_default(),
                format::duration(w.duration),
                format::distance(w.distance),
            ),
            None => (
                "(unknown workout)".to_owned(),
                MISSING.to_owned(),
                MISSING.to_owned(),
            ),
        };
        let done = match (&e.activity_id, e.completion) {
            (Some(id), Some(c)) => format!("done {:.0}% (activity {id})", c * 100.0),
            (Some(id), None) => format!("done (activity {id})"),
            _ => String::new(),
        };
        println!(
            "{}  {}  {:<24}  {:>8}  {:>9}  {done}",
            e.date,
            e.date.format("%a"),
            fit(&name, 24),
            time,
            dist
        );
    }
    if !s.weeks.is_empty() {
        println!("\nWeek of      Distance plan / done      Time plan / done     Load plan / done");
        for w in &s.weeks {
            println!(
                "{}   {:>9} / {:<9}     {:>8} / {:<8}    {:>4} / {}",
                w.week_start,
                format::distance(w.plan_distance),
                format::distance(w.actual_distance),
                format::duration(w.plan_duration),
                format::duration(w.actual_duration),
                format::opt_f(w.plan_load, 0),
                format::opt_f(w.actual_load, 0)
            );
        }
    }
}

fn print_workouts(workouts: &[Workout]) {
    if workouts.is_empty() {
        println!("Your workout library is empty.");
    }
    for w in workouts {
        println!(
            "{}  {}  (sport code {}, ~{}, {})",
            w.id,
            w.name.as_deref().unwrap_or("(no name)"),
            format::opt(w.sport_code),
            format::duration(w.estimated_time),
            format::distance(w.distance)
        );
        for step in &w.steps {
            print_step(step, 1);
        }
    }
}

fn print_step(step: &WorkoutStep, depth: usize) {
    let indent = "    ".repeat(depth);
    if step.kind == Some(StepKind::Group) || !step.steps.is_empty() {
        println!("{indent}{}× {}", step.sets, step.label);
        for child in &step.steps {
            print_step(child, depth + 1);
        }
        return;
    }
    let target = match &step.target {
        StepTarget::Distance { meters } => format::distance(Some(*meters)),
        StepTarget::Time { seconds } => {
            format::duration(Some(std::time::Duration::from_secs_f64(seconds.max(0.0))))
        }
        StepTarget::Open => "until lap".to_owned(),
        StepTarget::Other { target_type, value } => {
            format!("type {target_type} = {}", format::opt_f(*value, 0))
        }
    };
    let intensity = match &step.intensity {
        Some(Intensity::Pace { fastest, slowest }) => format!(
            " @ {}–{}",
            format::pace(Some(*fastest), SportCategory::Run).trim_end_matches(" /km"),
            format::pace(Some(*slowest), SportCategory::Run)
        ),
        Some(Intensity::Other {
            intensity_type,
            low,
            high,
        }) => format!(
            " @ type {intensity_type}: {}–{}",
            format::opt_f(*low, 0),
            format::opt_f(*high, 0)
        ),
        None => String::new(),
    };
    let sets = if step.sets > 1 {
        format!("{}× ", step.sets)
    } else {
        String::new()
    };
    println!("{indent}{sets}{} {target}{intensity}", step.label);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 18).unwrap();
        let none = RangeArgs::default();
        assert_eq!(optional_range(&none, today, 28).unwrap(), None);
        let r = required_range(&none, today, 28).unwrap();
        assert_eq!(
            (r.start, r.end, r.days()),
            (NaiveDate::from_ymd_opt(2026, 9, 21).unwrap(), today, 28)
        );
        let backwards = RangeArgs {
            from: Some(today),
            to: NaiveDate::from_ymd_opt(2026, 1, 1),
        };
        assert!(required_range(&backwards, today, 7).is_err());
    }

    #[test]
    fn home_expansion() {
        let p = expand_home(std::path::Path::new("~/Downloads"));
        assert!(!p.starts_with("~"), "{}", p.display());
        assert_eq!(
            expand_home(std::path::Path::new("/tmp/x")),
            PathBuf::from("/tmp/x")
        );
    }
}
