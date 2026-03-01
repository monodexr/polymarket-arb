/// Fair value for a 5-minute up/down binary contract.
///
/// Uses elapsed time (not remaining) for more intuitive certainty scaling.
/// Certainty = 0.3 at window open → 1.0 at window close.
/// Shift = move_pct * 500 * certainty, clamped to ±0.45.
pub fn fair_yes(spot_now: f64, open_price: f64, time_remaining_frac: f64) -> f64 {
    if open_price <= 0.0 || spot_now <= 0.0 {
        return 0.50;
    }

    if time_remaining_frac < 0.0 {
        return 0.50;
    }

    let move_pct = (spot_now - open_price) / open_price;

    let elapsed_frac = (1.0 - time_remaining_frac).clamp(0.0, 1.0);
    let certainty = 0.3 + 0.7 * elapsed_frac;

    let shift = (move_pct * 80.0 * certainty).clamp(-0.45, 0.45);

    (0.50 + shift).clamp(0.05, 0.95)
}

pub fn fair_no(spot_now: f64, open_price: f64, time_remaining_frac: f64) -> f64 {
    1.0 - fair_yes(spot_now, open_price, time_remaining_frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_market_at_start() {
        let fv = fair_yes(84000.0, 84000.0, 1.0);
        assert!((fv - 0.50).abs() < 0.01, "flat at open should be ~0.50, got {fv}");
    }

    #[test]
    fn flat_market_midway() {
        let fv = fair_yes(84000.0, 84000.0, 0.5);
        assert!((fv - 0.50).abs() < 0.01, "flat mid-window should be ~0.50, got {fv}");
    }

    #[test]
    fn up_01pct_midway() {
        // +0.1% at halfway: certainty=0.3+0.7*0.5=0.65, shift=0.001*500*0.65=0.325
        let fv = fair_yes(84084.0, 84000.0, 0.5);
        assert!(fv > 0.52, "+0.1% mid should be >0.52, got {fv}");
        assert!(fv < 0.60, "+0.1% mid should be <0.60, got {fv}");
    }

    #[test]
    fn down_02pct_near_end() {
        // -0.2% at t=240s (time_remaining_frac=0.2): certainty=0.3+0.7*0.8=0.86
        // shift = -0.002*500*0.86 = -0.86 → clamped to -0.45 → 0.05? No:
        // shift = -0.86 → clamp(-0.45,0.45) = -0.45 → fair = 0.05
        // Actually: -0.002*500 = -1.0, *0.86 = -0.86, clamp = -0.45 → 0.05
        // That's too extreme. Let's check a milder move.
        // -0.2% at t=240s: move_pct = -0.002
        let fv = fair_yes(83832.0, 84000.0, 0.2);
        assert!(fv < 0.40, "-0.2% near end should be <0.40, got {fv}");
    }

    #[test]
    fn symmetry() {
        let up = fair_yes(84100.0, 84000.0, 0.5);
        let down = fair_yes(83900.0, 84000.0, 0.5);
        assert!((up + down - 1.0).abs() < 0.02, "should be symmetric: up={up}, down={down}");
    }

    #[test]
    fn extreme_move_late() {
        let fv = fair_yes(84500.0, 84000.0, 0.067); // ~280s elapsed of 300s
        assert!(fv > 0.85, "big up move late should be >0.85, got {fv}");
        let fv2 = fair_yes(83500.0, 84000.0, 0.067);
        assert!(fv2 < 0.15, "big down move late should be <0.15, got {fv2}");
    }

    #[test]
    fn complement() {
        let yes = fair_yes(84252.0, 84000.0, 0.5);
        let no = fair_no(84252.0, 84000.0, 0.5);
        assert!((yes + no - 1.0).abs() < 0.001, "YES+NO should = 1.0, got {}", yes + no);
    }

    #[test]
    fn expired_returns_half() {
        let fv = fair_yes(85000.0, 84000.0, -0.1);
        assert_eq!(fv, 0.50);
    }

    #[test]
    fn small_move_still_prices() {
        // 0.05% move: shift = 0.0005*80*0.65 = 0.026 → fair ≈ 0.526
        let fv = fair_yes(84042.0, 84000.0, 0.5);
        assert!(fv > 0.50, "small up move should be >0.50, got {fv}");
        assert!(fv < 0.56, "small up move should be <0.56, got {fv}");
    }

    #[test]
    fn late_window_more_certain() {
        let early = fair_yes(84168.0, 84000.0, 0.9);
        let late = fair_yes(84168.0, 84000.0, 0.1);
        assert!(late > early, "late should be more certain: early={early}, late={late}");
    }
}
