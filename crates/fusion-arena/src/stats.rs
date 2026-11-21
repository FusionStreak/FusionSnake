//! Match tallies and confidence intervals.

use serde::Serialize;

/// Two-sided 95% normal quantile.
const Z_95: f64 = 1.959_964;

/// Results collected for one matchup.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Tally {
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    /// Games that produced no usable result. Excluded from every rate.
    pub failed: u32,
    /// Move requests that failed or timed out, across all snakes.
    pub request_failures: u32,
}

impl Tally {
    /// Number of games with a usable result.
    #[must_use]
    pub fn played(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    /// Points per game, counting a draw as half a win.
    #[must_use]
    pub fn score(&self) -> f64 {
        let played = self.played();
        if played == 0 {
            return 0.0;
        }
        (f64::from(self.wins) + f64::from(self.draws) * 0.5) / f64::from(played)
    }

    /// 95% Wilson interval around [`Tally::score`].
    #[must_use]
    pub fn interval(&self) -> (f64, f64) {
        let played = self.played();
        if played == 0 {
            return (0.0, 1.0);
        }
        let n = f64::from(played);
        let p = self.score();
        let z2 = Z_95 * Z_95;
        let denominator = 1.0 + z2 / n;
        let centre = (p + z2 / (2.0 * n)) / denominator;
        let margin = Z_95 * (p * (1.0 - p) / n + z2 / (4.0 * n * n)).sqrt() / denominator;
        ((centre - margin).max(0.0), (centre + margin).min(1.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_count_as_half_a_win() {
        let tally = Tally {
            wins: 1,
            draws: 2,
            losses: 1,
            ..Tally::default()
        };
        assert!((tally.score() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn failed_games_are_excluded() {
        let tally = Tally {
            wins: 1,
            failed: 9,
            ..Tally::default()
        };
        assert_eq!(tally.played(), 1);
        assert!((tally.score() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn wilson_interval_matches_known_value() {
        let tally = Tally {
            wins: 50,
            losses: 50,
            ..Tally::default()
        };
        let (low, high) = tally.interval();
        assert!((low - 0.4038).abs() < 0.001, "low was {low}");
        assert!((high - 0.5962).abs() < 0.001, "high was {high}");
    }

    #[test]
    fn empty_tally_has_uninformative_interval() {
        assert_eq!(Tally::default().interval(), (0.0, 1.0));
    }
}
