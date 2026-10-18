//! Activity detail tabs added in Phase 3: Charts, Map, Notes.

use coros_api::{
    ActivityDetail, ActivitySummary, PerceivedExertion, Sample, SportCategory, SportType,
};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Line as CanvasLine};
use ratatui::widgets::{Axis, Chart, Dataset, GraphType, Paragraph, Wrap};

use crate::charts::{labels, lttb, median, padded_bounds};
use crate::format;
use crate::theme::Theme;
use crate::widgets::message;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Metric {
    HeartRate,
    Pace,
    Elevation,
    Cadence,
    Power,
}

impl Metric {
    pub const ALL: [Metric; 5] = [
        Metric::HeartRate,
        Metric::Pace,
        Metric::Elevation,
        Metric::Cadence,
        Metric::Power,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::HeartRate => "Heart rate",
            Self::Pace => "Pace",
            Self::Elevation => "Elevation",
            Self::Cadence => "Cadence",
            Self::Power => "Power",
        }
    }

    fn short(self) -> &'static str {
        match self {
            Self::HeartRate => "HR",
            Self::Pace => "Pace",
            Self::Elevation => "Elev",
            Self::Cadence => "Cad",
            Self::Power => "Pwr",
        }
    }

    fn value(self, s: &Sample) -> Option<f64> {
        match self {
            Self::HeartRate => s.heart_rate.map(f64::from),
            Self::Pace => s.pace.map(|p| p.secs_per_km()),
            Self::Elevation => s.altitude,
            Self::Cadence => s.cadence.map(f64::from),
            Self::Power => s.power.map(f64::from),
        }
    }

    fn format(self, v: f64) -> String {
        match self {
            Self::HeartRate => format!("{v:.0}"),
            Self::Pace => {
                format::pace(Some(coros_api::Pace(v.abs())), SportCategory::Run).replace(" /km", "")
            }
            Self::Elevation => format!("{v:.0} m"),
            Self::Cadence => format!("{v:.0}"),
            Self::Power => format!("{v:.0} W"),
        }
    }

    fn unit(self) -> &'static str {
        match self {
            Self::HeartRate => "bpm",
            Self::Pace => "/km",
            Self::Elevation => "m",
            Self::Cadence => "spm",
            Self::Power => "W",
        }
    }
}

/// Metrics with enough samples to draw.
pub fn available_metrics(detail: &ActivityDetail) -> Vec<Metric> {
    Metric::ALL
        .into_iter()
        .filter(|m| {
            detail
                .samples
                .iter()
                .filter(|s| m.value(s).is_some())
                .count()
                >= 2
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XAxis {
    Distance,
    Time,
}

/// (x, y) points for `metric` against `axis`. Distance falls back to time when
/// the samples carry no distance (treadmill without a footpod, strength).
fn series(detail: &ActivityDetail, metric: Metric, axis: XAxis) -> (Vec<(f64, f64)>, XAxis) {
    let t0 = detail.samples.iter().find_map(|s| s.time);
    let by_distance = axis == XAxis::Distance
        && detail
            .samples
            .iter()
            .filter(|s| s.distance.is_some())
            .count()
            >= 2;
    let axis = if by_distance {
        XAxis::Distance
    } else {
        XAxis::Time
    };
    let mut points: Vec<(f64, f64)> = detail
        .samples
        .iter()
        .filter_map(|s| {
            let x = match axis {
                XAxis::Distance => s.distance?.km(),
                XAxis::Time => (s.time? - t0?).num_milliseconds() as f64 / 60_000.0,
            };
            Some((x, metric.value(s)?))
        })
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    if metric == Metric::Pace {
        // Stops make the pace explode (18 000 s/km); cap at 2.5× the median.
        let cap =
            median(&points.iter().map(|p| p.1).collect::<Vec<_>>()).map_or(f64::MAX, |m| m * 2.5);
        for p in &mut points {
            // Plot negated so faster is higher; labels show the absolute pace.
            p.1 = -p.1.min(cap);
        }
    }
    (points, axis)
}

pub fn render_charts(
    frame: &mut Frame,
    area: Rect,
    detail: &ActivityDetail,
    wanted: Metric,
    axis: XAxis,
    theme: &Theme,
) {
    let metrics = available_metrics(detail);
    let Some(&metric) = metrics
        .iter()
        .find(|m| **m == wanted)
        .or_else(|| metrics.first())
    else {
        return message(
            frame,
            area,
            vec![Line::from(Span::styled(
                "No per-second data for this activity.",
                theme.muted,
            ))],
        );
    };
    let (raw, axis) = series(detail, metric, axis);
    let [header, chart_area] =
        Layout::vertical([Constraint::Length(2), Constraint::Fill(1)]).areas(area);

    let ys: Vec<f64> = raw.iter().map(|p| p.1).collect();
    let mean = ys.iter().sum::<f64>() / ys.len().max(1) as f64;
    // Highest value: the max, or for (negated) pace the fastest.
    let extreme = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let mut picker: Vec<Span> = vec![Span::styled(" ", theme.text)];
    for m in &metrics {
        // Brackets as well as colour, so it reads without colour too.
        let (style, text) = if *m == metric {
            (theme.tab_active, format!("[{}]", m.short()))
        } else {
            (theme.muted, format!(" {} ", m.short()))
        };
        picker.push(Span::styled(text, style));
    }
    picker.push(Span::styled(
        format!(
            "   m: metric · x: {} ",
            if axis == XAxis::Distance {
                "distance"
            } else {
                "time"
            }
        ),
        theme.muted,
    ));
    let stats = Line::from(vec![
        Span::styled(
            format!(" {} ({})", metric.label(), metric.unit()),
            theme.header,
        ),
        Span::styled(
            format!(
                "   avg {} · {} {}",
                metric.format(mean),
                if metric == Metric::Pace {
                    "fastest"
                } else {
                    "max"
                },
                metric.format(extreme)
            ),
            theme.muted,
        ),
    ]);
    frame.render_widget(Paragraph::new(vec![Line::from(picker), stats]), header);

    let points = lttb(&raw, usize::from(chart_area.width.max(10)) * 2);
    let max_x = points.last().map_or(1.0, |p| p.0).max(0.01);
    let (lo, hi) = padded_bounds(points.iter().map(|p| p.1)).unwrap_or((0.0, 1.0));
    let x_fmt = |v: f64| match axis {
        XAxis::Distance => format!("{v:.1} km"),
        XAxis::Time => format!("{v:.0} min"),
    };
    let style = match metric {
        Metric::HeartRate => theme.zones[5],
        Metric::Pace => theme.accent,
        Metric::Elevation => theme.zones[1],
        Metric::Cadence => theme.zones[2],
        Metric::Power => theme.zones[4],
    };
    let chart = Chart::new(vec![
        Dataset::default()
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(style)
            .data(&points),
    ])
    .x_axis(
        Axis::default()
            .style(theme.muted)
            .bounds([0.0, max_x])
            .labels(labels(0.0, max_x, 3, x_fmt)),
    )
    .y_axis(
        Axis::default()
            .style(theme.muted)
            .bounds([lo, hi])
            .labels(labels(lo, hi, 3, |v| metric.format(v))),
    );
    frame.render_widget(chart, chart_area);
}

/// Project lat/lon onto a flat plane: longitude shrinks with cos(latitude).
fn project(samples: &[Sample]) -> Vec<(f64, f64)> {
    let track: Vec<(f64, f64)> = samples
        .iter()
        .filter_map(|s| Some((s.lat?, s.lon?)))
        .collect();
    let lat0 = track.iter().map(|p| p.0).sum::<f64>() / track.len().max(1) as f64;
    let k = lat0.to_radians().cos();
    track.into_iter().map(|(lat, lon)| (lon * k, lat)).collect()
}

/// Bounds that fit the track and keep its shape: a braille dot is about as
/// wide as it is tall when a cell is twice as tall as wide (2×4 dots per cell).
fn fit_bounds(points: &[(f64, f64)], width: u16, height: u16) -> ([f64; 2], [f64; 2]) {
    let (x0, x1) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| {
            (a.0.min(p.0), a.1.max(p.0))
        });
    let (y0, y1) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |a, p| {
            (a.0.min(p.1), a.1.max(p.1))
        });
    let (dots_w, dots_h) = (
        f64::from(width.max(1)) * 2.0,
        f64::from(height.max(1)) * 4.0,
    );
    let per_dot = ((x1 - x0) / dots_w).max((y1 - y0) / dots_h).max(1e-9) * 1.05;
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    (
        [cx - per_dot * dots_w / 2.0, cx + per_dot * dots_w / 2.0],
        [cy - per_dot * dots_h / 2.0, cy + per_dot * dots_h / 2.0],
    )
}

pub fn render_map(frame: &mut Frame, area: Rect, detail: &ActivityDetail, theme: &Theme) {
    let points = project(&detail.samples);
    if points.len() < 2 {
        return message(
            frame,
            area,
            vec![Line::from(Span::styled(
                "No GPS track (indoor activity, or GPS was off).",
                theme.muted,
            ))],
        );
    }
    let [header, map_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    let s = &detail.summary;
    frame.render_widget(
        Line::from(vec![
            Span::styled(" S", theme.accent),
            Span::styled(" start · ", theme.muted),
            Span::styled("F", theme.error),
            Span::styled(" finish · north is up", theme.muted),
            Span::styled(
                format!(
                    "   {} · +{} m",
                    format::distance(s.distance),
                    format::opt_f(s.elevation_gain.map(|m| m.0), 0)
                ),
                theme.text,
            ),
        ]),
        header,
    );
    let (xb, yb) = fit_bounds(&points, map_area.width, map_area.height);
    let colour = |style: Style| style.fg.unwrap_or(Color::Reset);
    let (track, start, finish) = (colour(theme.zones[2]), theme.accent, theme.error);
    let (first, last) = (points[0], points[points.len() - 1]);
    // On a loop the finish would hide the start: draw one marker.
    let extent = (xb[1] - xb[0]).max(yb[1] - yb[0]);
    let looped = ((first.0 - last.0).powi(2) + (first.1 - last.1).powi(2)).sqrt() < extent * 0.03;
    let canvas = Canvas::default()
        .marker(Marker::Braille)
        .x_bounds(xb)
        .y_bounds(yb)
        .paint(move |ctx| {
            for w in points.windows(2) {
                ctx.draw(&CanvasLine {
                    x1: w[0].0,
                    y1: w[0].1,
                    x2: w[1].0,
                    y2: w[1].1,
                    color: track,
                });
            }
            ctx.layer();
            if looped {
                ctx.print(first.0, first.1, Span::styled("S/F", start));
            } else {
                ctx.print(first.0, first.1, Span::styled("S", start));
                ctx.print(last.0, last.1, Span::styled("F", finish));
            }
        });
    frame.render_widget(canvas, map_area);
}

fn compass(degrees: f64) -> &'static str {
    const POINTS: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW",
        "NW", "NNW",
    ];
    POINTS[((degrees.rem_euclid(360.0) / 22.5).round() as usize) % 16]
}

pub fn render_notes(
    frame: &mut Frame,
    area: Rect,
    detail: &ActivityDetail,
    summary: &ActivitySummary,
    theme: &Theme,
) {
    let row = |label: &str, value: Vec<Span<'static>>| {
        let mut spans = vec![Span::styled(format!(" {label:<12}"), theme.muted)];
        spans.extend(value);
        Line::from(spans)
    };
    let mut lines = Vec::new();

    let scale = [
        PerceivedExertion::VeryLight,
        PerceivedExertion::Light,
        PerceivedExertion::Moderate,
        PerceivedExertion::Hard,
        PerceivedExertion::MaxEffort,
    ];
    let effort = match detail.feel.perceived_exertion {
        None => vec![Span::styled("not rated", theme.muted)],
        Some(rpe) => scale
            .iter()
            .enumerate()
            .flat_map(|(i, level)| {
                let sep = if i == 0 { "" } else { " · " };
                let (style, text) = if *level == rpe {
                    (theme.tab_active, format!("[{}]", level.label()))
                } else {
                    (theme.muted, format!(" {} ", level.label()))
                };
                [Span::styled(sep, theme.muted), Span::styled(text, style)]
            })
            .collect(),
    };
    lines.push(row("Effort", effort));
    lines.push(Line::from(""));
    match &detail.feel.note {
        Some(note) => {
            lines.push(row("Note", vec![]));
            for l in note.lines() {
                lines.push(Line::from(Span::styled(format!("   {l}"), theme.text)));
            }
        }
        None => lines.push(row("Note", vec![Span::styled("no note", theme.muted)])),
    }
    lines.push(Line::from(""));

    if let Some(w) = &detail.weather {
        let mut parts = Vec::new();
        if let Some(t) = w.temperature_c {
            parts.push(format!("{t:.1} °C"));
        }
        if let Some(f) = w.feels_like_c {
            parts.push(format!("feels {f:.1} °C"));
        }
        if let Some(h) = w.humidity_pct {
            parts.push(format!("humidity {h:.0}%"));
        }
        if let Some(s) = w.wind_speed {
            let dir = w
                .wind_direction_deg
                .map_or_else(String::new, |d| format!(" from {} ({d:.0}°)", compass(d)));
            parts.push(format!("wind {s:.1}{dir}"));
        }
        lines.push(row(
            "Weather",
            vec![Span::styled(parts.join(" · "), theme.text)],
        ));
    } else {
        lines.push(row(
            "Weather",
            vec![Span::styled("not recorded", theme.muted)],
        ));
    }
    let s = &detail.summary;
    if let (Some(start), Some(end)) = (s.start, s.end) {
        lines.push(row(
            "Time",
            vec![Span::styled(
                format!(
                    "{} – {} (UTC{})",
                    start.format("%H:%M"),
                    end.format("%H:%M"),
                    start.format("%:z")
                ),
                theme.text,
            )],
        ));
    }
    if let Some(device) = &summary.device {
        lines.push(row(
            "Device",
            vec![Span::styled(device.clone(), theme.text)],
        ));
    }
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
}

/// Sport-aware wording for the chart picker (pace only for run-like sports).
pub fn default_metric(sport: SportType) -> Metric {
    match sport.category() {
        SportCategory::Run | SportCategory::Walk => Metric::Pace,
        _ => Metric::HeartRate,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testdata;

    #[test]
    fn metrics_come_from_the_samples() {
        let d = testdata::detail();
        assert_eq!(available_metrics(&d), Metric::ALL.to_vec());
        let mut empty = d.clone();
        empty.samples.clear();
        assert!(available_metrics(&empty).is_empty());
    }

    #[test]
    fn pace_series_is_inverted_and_capped() {
        let mut d = testdata::detail();
        d.samples[3].pace = Some(coros_api::Pace(18_000.0)); // a stop
        let (points, axis) = series(&d, Metric::Pace, XAxis::Distance);
        assert_eq!(axis, XAxis::Distance);
        assert!(
            points.iter().all(|p| p.1 < 0.0),
            "negated: faster is higher"
        );
        let slowest = points.iter().map(|p| -p.1).fold(0.0, f64::max);
        assert!(slowest < 1000.0, "stop capped, got {slowest}");
        assert!(points.windows(2).all(|w| w[0].0 <= w[1].0));
    }

    #[test]
    fn distance_axis_falls_back_to_time() {
        let mut d = testdata::detail();
        for s in &mut d.samples {
            s.distance = None;
        }
        let (_, axis) = series(&d, Metric::HeartRate, XAxis::Distance);
        assert_eq!(axis, XAxis::Time);
    }

    #[test]
    fn map_bounds_keep_the_aspect_ratio() {
        // A track twice as wide as it is tall, drawn in a 40×10 cell area (80×40 dots).
        let points = [(0.0, 0.0), (2.0, 1.0)];
        let (xb, yb) = fit_bounds(&points, 40, 10);
        let per_dot_x = (xb[1] - xb[0]) / 80.0;
        let per_dot_y = (yb[1] - yb[0]) / 40.0;
        assert!((per_dot_x - per_dot_y).abs() < 1e-9, "square dots");
        assert!(
            xb[0] <= 0.0 && xb[1] >= 2.0 && yb[0] <= 0.0 && yb[1] >= 1.0,
            "track fits"
        );
    }

    #[test]
    fn compass_points() {
        assert_eq!(compass(0.0), "N");
        assert_eq!(compass(338.0), "NNW");
        assert_eq!(compass(359.0), "N");
        assert_eq!(compass(90.0), "E");
    }
}
