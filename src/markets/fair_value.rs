use statrs::distribution::{ContinuousCDF, Normal};

/// Black-Scholes fair value for a binary "BTC > K by T" contract.
/// Returns probability of finishing above strike = Phi(d2).
pub fn binary_fair_value(spot: f64, strike: f64, time_years: f64, vol: f64, rate: f64) -> f64 {
    if time_years <= 0.0 {
        return if spot > strike { 1.0 } else { 0.0 };
    }

    if vol <= 0.0 || strike <= 0.0 || spot <= 0.0 {
        return 0.0;
    }

    let d2 = ((spot / strike).ln() + (rate - vol * vol / 2.0) * time_years)
        / (vol * time_years.sqrt());

    let norm = Normal::new(0.0, 1.0).unwrap();
    let fair = norm.cdf(d2);

    fair.clamp(0.001, 0.999)
}

/// Time to expiry in fractional years.
pub fn time_to_expiry_years(expiry: chrono::DateTime<chrono::Utc>) -> f64 {
    let now = chrono::Utc::now();
    let duration = expiry - now;
    let secs = duration.num_seconds().max(0) as f64;
    secs / (365.25 * 24.0 * 3600.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deep_itm() {
        let fv = binary_fair_value(100_000.0, 50_000.0, 0.1, 0.65, 0.05);
        assert!(fv > 0.95, "deep ITM should be near 1.0, got {fv}");
    }

    #[test]
    fn test_deep_otm() {
        let fv = binary_fair_value(50_000.0, 200_000.0, 0.01, 0.65, 0.05);
        assert!(fv < 0.05, "deep OTM should be near 0.0, got {fv}");
    }

    #[test]
    fn test_atm() {
        let fv = binary_fair_value(100_000.0, 100_000.0, 0.1, 0.65, 0.05);
        assert!(
            (0.4..=0.6).contains(&fv),
            "ATM should be near 0.5, got {fv}"
        );
    }

    #[test]
    fn test_expired() {
        assert_eq!(binary_fair_value(100_000.0, 90_000.0, 0.0, 0.65, 0.05), 1.0);
        assert_eq!(binary_fair_value(80_000.0, 90_000.0, 0.0, 0.65, 0.05), 0.0);
    }
}
