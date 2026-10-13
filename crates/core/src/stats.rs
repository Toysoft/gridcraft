//! Order statistics shared by the worksheet functions (PERCENTILE, QUARTILE), conditional
//! formatting thresholds and box & whisker charts, so they all compute the same values.

/// The value at the 0-based position `pos` of ascending `sorted`, interpolating linearly between
/// neighbours (a position between n−1 and n gives the last value). `None` for an empty slice, a
/// negative or non-finite position, or one at or past n.
pub fn interpolate(sorted: &[f64], pos: f64) -> Option<f64> {
    if !pos.is_finite() || pos < 0.0 {
        return None;
    }
    let i = pos.floor();
    let frac = pos - i;
    let i = i as usize;
    let lo = *sorted.get(i)?;
    match sorted.get(i + 1) {
        Some(hi) if frac > 0.0 => Some(lo + frac * (hi - lo)),
        _ => Some(lo),
    }
}

/// PERCENTILE.INC: the `k`-th percentile (0 ≤ k ≤ 1) of ascending `sorted`, at position k·(n−1).
/// `None` when `sorted` is empty or `k` is out of range.
pub fn percentile_inc(sorted: &[f64], k: f64) -> Option<f64> {
    if sorted.is_empty() || !(0.0..=1.0).contains(&k) {
        return None;
    }
    interpolate(sorted, k * (sorted.len() - 1) as f64)
}

/// PERCENTILE.EXC: the `k`-th percentile (0 < k < 1) of ascending `sorted`, at the 1-based
/// position k·(n+1). `None` when `sorted` is empty, `k` is out of range, or the position falls
/// outside 1..=n.
pub fn percentile_exc(sorted: &[f64], k: f64) -> Option<f64> {
    let n = sorted.len() as f64;
    if sorted.is_empty() || !(k > 0.0 && k < 1.0) {
        return None;
    }
    let pos = k * (n + 1.0);
    if pos < 1.0 || pos > n {
        return None;
    }
    interpolate(sorted, pos - 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_match_excel() {
        let s = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0];
        assert_eq!(percentile_inc(&s, 0.25), Some(2.25));
        assert_eq!(percentile_inc(&s, 0.75), Some(4.75));
        assert_eq!(percentile_exc(&s, 0.25), Some(1.75));
        assert_eq!(percentile_exc(&s, 0.75), Some(5.25));
        assert_eq!(percentile_inc(&s, 0.0), Some(1.0));
        assert_eq!(percentile_inc(&s, 1.0), Some(6.0));
        assert_eq!(percentile_exc(&[1.0, 2.0], 0.25), None);
        assert_eq!(percentile_inc(&[], 0.5), None);
        assert_eq!(percentile_inc(&s, 1.5), None);
        assert_eq!(interpolate(&s, f64::NAN), None);
        assert_eq!(interpolate(&s, 9.0), None);
    }
}
