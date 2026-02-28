/// Fair value for a 5-minute up/down binary contract.
///
/// Based on how much the spot price has moved from the window opening price
/// and how much time remains. Conservative model backed by pawn shop data:
/// >0.20% moves predict direction with ~99% accuracy.
pub fn fair_yes(spot_now: f64, open_price: f64, time_remaining_frac: f64) -> f64 {
    if open_price <= 0.0 || spot_now <= 0.0 {
        return 0.50;
    }

    // Past window close — don't price expired windows
    if time_remaining_frac < 0.0 {
        return 0.50;
    }

    let move_pct = (spot_now - open_price) / open_price;

    // Below 0.1% move: noise, no signal
    if move_pct.abs() < 0.001 {
        return 0.50;
    }

    // Certainty scales from 0.5 at window open to 1.0 at window close.
    // Later in the window = more confident the direction is locked.
    let certainty = (1.0 - time_remaining_frac) * 0.5 + 0.5;

    let fair = if move_pct > 0.0 {
        0.50 + (move_pct * 100.0 * certainty).min(0.45)
    } else {
        0.50 - (move_pct.abs() * 100.0 * certainty).min(0.45)
    };

    fair.clamp(0.05, 0.95)
}

pub fn fair_no(spot_now: f64, open_price: f64, time_remaining_frac: f64) -> f64 {
    1.0 - fair_yes(spot_now, open_price, time_remaining_frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_move_returns_half() {
        assert_eq!(fair_yes(84000.0, 84000.0, 0.5), 0.50);
    }

    #[test]
    fn small_move_returns_half() {
        // 0.05% move — below 0.1% threshold
        let fv = fair_yes(84042.0, 84000.0, 0.5);
        assert_eq!(fv, 0.50);
    }

    #[test]
    fn up_move_increases_fair() {
        // 0.3% up move, mid-window
        let fv = fair_yes(84252.0, 84000.0, 0.5);
        assert!(fv > 0.55, "0.3% up should push fair above 0.55, got {fv}");
        assert!(fv < 0.80, "shouldn't be extreme, got {fv}");
    }

    #[test]
    fn down_move_decreases_fair() {
        // 0.3% down move, mid-window
        let fv = fair_yes(83748.0, 84000.0, 0.5);
        assert!(fv < 0.45, "0.3% down should push fair below 0.45, got {fv}");
        assert!(fv > 0.20, "shouldn't be extreme, got {fv}");
    }

    #[test]
    fn late_window_more_certain() {
        // Same 0.2% move, early vs late
        let early = fair_yes(84168.0, 84000.0, 0.9); // 90% time left
        let late = fair_yes(84168.0, 84000.0, 0.1);  // 10% time left
        assert!(late > early, "late window should be more certain: early={early}, late={late}");
    }

    #[test]
    fn large_move_caps_at_95() {
        // 2% move — should cap at 0.95
        let fv = fair_yes(85680.0, 84000.0, 0.1);
        assert_eq!(fv, 0.95);
    }

    #[test]
    fn expired_returns_half() {
        let fv = fair_yes(85000.0, 84000.0, -0.1);
        assert_eq!(fv, 0.50);
    }

    #[test]
    fn fair_no_complement() {
        let yes = fair_yes(84252.0, 84000.0, 0.5);
        let no = fair_no(84252.0, 84000.0, 0.5);
        assert!((yes + no - 1.0).abs() < 0.001, "YES + NO should = 1.0");
    }
}
