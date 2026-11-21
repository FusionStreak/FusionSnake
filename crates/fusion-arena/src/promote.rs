//! Decides whether candidate params should replace the champion.
//!
//! Duels run as a sequential probability ratio test (SPRT), which stops as
//! soon as the evidence is strong enough. A separate 4-player match then
//! rejects candidates that are clearly worse in multiplayer.

use std::path::PathBuf;

use crate::gauntlet::Mode;
use crate::matches;
use crate::stats::Tally;
use crate::worker::EngineSpec;

/// Duel score assumed if the candidate is no better.
const NULL_SCORE: f64 = 0.50;
/// Duel score the test is designed to detect.
const TARGET_SCORE: f64 = 0.52;
/// False-promotion and missed-improvement rates.
const ERROR_RATE: f64 = 0.05;
/// Score of an even 4-player result.
const FOUR_PLAYER_EVEN: f64 = 0.25;

/// State of the sequential test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Accept,
    Reject,
    Continue,
}

/// Log-likelihood ratio of a tally. Positive favours the target score. A draw
/// counts as half a win and half a loss.
#[must_use]
pub fn log_likelihood_ratio(tally: &Tally) -> f64 {
    let win = (TARGET_SCORE / NULL_SCORE).ln();
    let loss = ((1.0 - TARGET_SCORE) / (1.0 - NULL_SCORE)).ln();
    f64::from(tally.wins) * win
        + f64::from(tally.losses) * loss
        + f64::from(tally.draws) * 0.5 * (win + loss)
}

/// Applies the SPRT stopping bounds.
#[must_use]
pub fn verdict(tally: &Tally) -> Verdict {
    let bound = ((1.0 - ERROR_RATE) / ERROR_RATE).ln();
    let ratio = log_likelihood_ratio(tally);
    if ratio >= bound {
        Verdict::Accept
    } else if ratio <= -bound {
        Verdict::Reject
    } else {
        Verdict::Continue
    }
}

/// Settings for a promotion test.
#[derive(clap::Args)]
pub struct PromoteArgs {
    /// Candidate params JSON.
    #[arg(long)]
    candidate: PathBuf,
    /// Current champion params JSON. Defaults to the engine's defaults.
    #[arg(long)]
    champion: Option<PathBuf>,
    /// Where to copy the candidate if it passes.
    #[arg(long)]
    out: PathBuf,
    /// Board evaluations allowed per move.
    #[arg(long, default_value_t = 5_000)]
    evaluations: u64,
    /// Stop the duel test after this many games.
    #[arg(long, default_value_t = 6_000)]
    max_games: u32,
    /// Duel seeds per batch. Each seed is played from both seats.
    #[arg(long, default_value_t = 25)]
    batch_seeds: u32,
    /// 4-player seeds. Each seed is played from all four seats.
    #[arg(long, default_value_t = 100)]
    four_seeds: u32,
    /// First seed. Use seeds the candidate was not tuned on.
    #[arg(long, default_value_t = 900_000_000)]
    base_seed: u64,
    /// Games to run at once. Defaults to half the CPU cores.
    #[arg(long)]
    threads: Option<usize>,
}

fn add(total: &mut Tally, other: Tally) {
    total.wins += other.wins;
    total.draws += other.draws;
    total.losses += other.losses;
    total.failed += other.failed;
}

/// Runs the test and copies the candidate to `out` only if it passes.
///
/// # Errors
/// Returns an error if the candidate is not promoted, games fail, or the
/// result cannot be written.
pub fn run(args: &PromoteArgs) -> Result<(), String> {
    let binary = std::env::current_exe().map_err(|e| e.to_string())?;
    let candidate = EngineSpec {
        binary: binary.clone(),
        params: Some(args.candidate.clone()),
        evaluations: args.evaluations,
    };
    let champion = EngineSpec {
        binary,
        params: args.champion.clone(),
        evaluations: args.evaluations,
    };
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let threads = args.threads.unwrap_or((cores / 2).max(1));

    let mut duel = Tally::default();
    let mut batch = 0_u64;
    let result = loop {
        let seed = args.base_seed + batch * u64::from(args.batch_seeds);
        let games = matches::pairings(Mode::Duel, args.batch_seeds, seed);
        add(
            &mut duel,
            matches::run(&candidate, &champion, Mode::Duel, &games, threads).tally,
        );
        batch += 1;
        if duel.failed > 0 {
            return Err(format!("{} duel games failed", duel.failed));
        }

        let current = verdict(&duel);
        println!(
            "duel {:>5} games  {}-{}-{}  score {:.3}  LLR {:+.2}",
            duel.played(),
            duel.wins,
            duel.draws,
            duel.losses,
            duel.score(),
            log_likelihood_ratio(&duel)
        );
        if current != Verdict::Continue {
            break current;
        }
        if duel.played() >= args.max_games {
            return Err("inconclusive: reached the game limit, keeping the champion".to_owned());
        }
    };
    if result == Verdict::Reject {
        return Err("rejected: candidate is not stronger in Duels".to_owned());
    }

    // Offset past every seed the duel test could have used.
    let four_seed = args.base_seed + u64::from(args.max_games) + u64::from(args.batch_seeds);
    let games = matches::pairings(Mode::Four, args.four_seeds, four_seed);
    let four = matches::run(&candidate, &champion, Mode::Four, &games, threads).tally;
    if four.failed > 0 {
        return Err(format!("{} 4-player games failed", four.failed));
    }
    let (low, high) = four.interval();
    println!(
        "4-player {} games  {}-{}-{}  score {:.3}  95% CI [{low:.3}, {high:.3}]",
        four.played(),
        four.wins,
        four.draws,
        four.losses,
        four.score()
    );
    if high < FOUR_PLAYER_EVEN {
        return Err("rejected: candidate is worse in 4-player games".to_owned());
    }

    if let Some(parent) = args.out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::copy(&args.candidate, &args.out).map_err(|e| e.to_string())?;
    println!("promoted: {}", args.out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tally(wins: u32, draws: u32, losses: u32) -> Tally {
        Tally {
            wins,
            draws,
            losses,
            ..Tally::default()
        }
    }

    #[test]
    fn even_results_do_not_decide() {
        assert_eq!(verdict(&tally(50, 0, 50)), Verdict::Continue);
    }

    #[test]
    fn clear_improvement_is_accepted() {
        assert_eq!(verdict(&tally(120, 0, 40)), Verdict::Accept);
    }

    #[test]
    fn clear_regression_is_rejected() {
        assert_eq!(verdict(&tally(40, 0, 120)), Verdict::Reject);
    }

    #[test]
    fn ratio_matches_hand_calculation() {
        let expected = 3.0 * 1.04_f64.ln() + 0.96_f64.ln();
        assert!((log_likelihood_ratio(&tally(3, 0, 1)) - expected).abs() < 1e-12);
    }

    #[test]
    fn draw_counts_as_half_a_win_and_half_a_loss() {
        let draws = log_likelihood_ratio(&tally(0, 2, 0));
        let split = log_likelihood_ratio(&tally(1, 0, 1));
        assert!((draws - split).abs() < 1e-12);
    }
}
