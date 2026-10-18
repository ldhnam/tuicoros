//! Helpers for ratatui charts: downsampling, bounds, axis labels.

use ratatui::text::Span;

/// Largest-Triangle-Three-Buckets: reduce `points` (sorted by x) to at most
/// `threshold` points while keeping the visual shape (peaks survive, unlike
/// plain averaging). Returns the input unchanged if it is already small.
pub fn lttb(points: &[(f64, f64)], threshold: usize) -> Vec<(f64, f64)> {
    let n = points.len();
    if threshold >= n || threshold < 3 {
        return points.to_vec();
    }
    // Everything but the first and last point is split into `threshold - 2` buckets.
    let every = (n - 2) as f64 / (threshold - 2) as f64;
    let bucket = |i: usize| -> (usize, usize) {
        let start = (i as f64 * every) as usize + 1;
        let end = (((i + 1) as f64 * every) as usize + 1).clamp(start + 1, n - 1);
        (start, end)
    };
    let mut out = Vec::with_capacity(threshold);
    out.push(points[0]);
    let mut a = 0;
    for i in 0..threshold - 2 {
        // Average of the next bucket (or the last point, for the final bucket).
        let (avg_x, avg_y) = if i + 1 < threshold - 2 {
            let (s, e) = bucket(i + 1);
            let next = &points[s..e];
            let sum = next
                .iter()
                .fold((0.0, 0.0), |acc, p| (acc.0 + p.0, acc.1 + p.1));
            (sum.0 / next.len() as f64, sum.1 / next.len() as f64)
        } else {
            points[n - 1]
        };
        // Keep the point in this bucket that forms the largest triangle.
        let (s, e) = bucket(i);
        let (ax, ay) = points[a];
        let mut best = s;
        let mut best_area = f64::NEG_INFINITY;
        for (j, &(x, y)) in points.iter().enumerate().take(e).skip(s) {
            let area = ((ax - avg_x) * (y - ay) - (ax - x) * (avg_y - ay)).abs();
            if area > best_area {
                best_area = area;
                best = j;
            }
        }
        out.push(points[best]);
        a = best;
    }
    out.push(points[n - 1]);
    out
}

/// Min and max with a little headroom, so lines don't sit on the border.
/// `None` for no finite values; a flat series gets ±1.
pub fn padded_bounds(values: impl IntoIterator<Item = f64>) -> Option<(f64, f64)> {
    let (lo, hi) = values
        .into_iter()
        .filter(|v| v.is_finite())
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
            (lo.min(v), hi.max(v))
        });
    if !lo.is_finite() {
        return None;
    }
    if (hi - lo).abs() < f64::EPSILON {
        return Some((lo - 1.0, hi + 1.0));
    }
    let pad = (hi - lo) * 0.08;
    Some((lo - pad, hi + pad))
}

/// `count` evenly spaced labels from `lo` to `hi`, formatted by `fmt`.
pub fn labels(lo: f64, hi: f64, count: usize, fmt: impl Fn(f64) -> String) -> Vec<Span<'static>> {
    let count = count.max(2);
    (0..count)
        .map(|i| Span::raw(fmt(lo + (hi - lo) * i as f64 / (count - 1) as f64)))
        .collect()
}

/// Median of the finite values (for outlier caps).
pub fn median(values: &[f64]) -> Option<f64> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lttb_keeps_ends_and_peaks() {
        let points: Vec<(f64, f64)> = (0..1000)
            .map(|i| (i as f64, if i == 500 { 100.0 } else { (i % 7) as f64 }))
            .collect();
        let out = lttb(&points, 50);
        assert_eq!(out.len(), 50);
        assert_eq!(out[0], points[0]);
        assert_eq!(out[49], points[999]);
        assert!(out.iter().any(|p| p.1 == 100.0), "the spike survives");
        assert!(out.windows(2).all(|w| w[0].0 < w[1].0), "x stays sorted");
        assert_eq!(lttb(&points[..10], 50).len(), 10, "small input untouched");
    }

    #[test]
    fn bounds_and_labels() {
        assert_eq!(padded_bounds([]), None);
        assert_eq!(padded_bounds([5.0, 5.0]), Some((4.0, 6.0)));
        let (lo, hi) = padded_bounds([0.0, 100.0, f64::NAN]).unwrap();
        assert!(lo < 0.0 && hi > 100.0);
        let l = labels(0.0, 10.0, 3, |v| format!("{v:.0}"));
        assert_eq!(
            l.iter().map(|s| s.content.to_string()).collect::<Vec<_>>(),
            ["0", "5", "10"]
        );
        assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
    }
}
