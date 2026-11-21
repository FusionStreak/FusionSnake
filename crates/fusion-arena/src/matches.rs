//! Reproducible in-process matches between two engine builds.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::gauntlet::Mode;
use crate::rules::{Finish, Game};
use crate::stats::Tally;
use crate::worker::{EngineSpec, Worker};

/// One game: the seed and which seat engine A occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pairing {
    pub seed: u64,
    pub a_seat: usize,
}

/// Plays every seed once with engine A in each seat.
#[must_use]
pub fn pairings(mode: Mode, seeds: u32, base_seed: u64) -> Vec<Pairing> {
    (0..u64::from(seeds))
        .flat_map(|offset| {
            (0..mode.seats()).map(move |a_seat| Pairing {
                seed: base_seed + offset,
                a_seat,
            })
        })
        .collect()
}

/// Aggregated results for engine A.
#[derive(Debug, Default)]
pub struct MatchReport {
    pub tally: Tally,
    pub a_depths: Vec<u8>,
}

impl MatchReport {
    /// Median search depth reached by engine A.
    #[must_use]
    pub fn median_depth(&self) -> Option<u8> {
        let mut depths = self.a_depths.clone();
        depths.sort_unstable();
        depths.get(depths.len() / 2).copied()
    }
}

fn play(
    a: &mut Worker,
    b: &mut Worker,
    mode: Mode,
    pairing: Pairing,
) -> Result<(Finish, Vec<u8>), String> {
    let mut game = Game::new(pairing.seed, mode.seats());
    let mut depths = Vec::new();

    loop {
        if let Some(finish) = game.finish() {
            return Ok((finish, depths));
        }
        let mut moves = Vec::with_capacity(game.snakes.len());
        for seat in 0..game.snakes.len() {
            if !game.snakes[seat].alive {
                moves.push(crate::rules::Move::Up);
            } else if seat == pairing.a_seat {
                let (mv, depth) = a.choose(&game, seat)?;
                depths.push(depth);
                moves.push(mv);
            } else {
                moves.push(b.choose(&game, seat)?.0);
            }
        }
        game.step(&moves);
    }
}

/// Runs a match across `threads` threads. Results do not depend on the thread
/// count, because each game is fully determined by its seed and seat.
///
/// # Panics
/// Panics if a worker thread panicked while holding the results lock.
#[must_use]
pub fn run(
    a: &EngineSpec,
    b: &EngineSpec,
    mode: Mode,
    games: &[Pairing],
    threads: usize,
) -> MatchReport {
    let next = AtomicUsize::new(0);
    let report = Mutex::new(MatchReport::default());

    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                let mut engines: Option<(Worker, Worker)> = None;
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&pairing) = games.get(index) else {
                        break;
                    };

                    let result = (|| {
                        if engines.is_none() {
                            engines = Some((Worker::spawn(a)?, Worker::spawn(b)?));
                        }
                        let (worker_a, worker_b) =
                            engines.as_mut().ok_or("workers are not running")?;
                        play(worker_a, worker_b, mode, pairing)
                    })();

                    let mut guard = report.lock().expect("results lock poisoned");
                    match result {
                        Ok((finish, depths)) => {
                            guard.a_depths.extend(depths);
                            match finish {
                                Finish::Draw => guard.tally.draws += 1,
                                Finish::Winner(seat) if seat == pairing.a_seat => {
                                    guard.tally.wins += 1;
                                }
                                Finish::Winner(_) => guard.tally.losses += 1,
                            }
                        }
                        Err(error) => {
                            guard.tally.failed += 1;
                            eprintln!("game {index} failed: {error}");
                            engines = None;
                        }
                    }
                }
            });
        }
    });
    report.into_inner().expect("results lock poisoned")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_seed_is_played_from_every_seat() {
        let games = pairings(Mode::Four, 2, 10);
        assert_eq!(games.len(), 8);
        assert_eq!(
            games[0],
            Pairing {
                seed: 10,
                a_seat: 0
            }
        );
        assert_eq!(
            games[7],
            Pairing {
                seed: 11,
                a_seat: 3
            }
        );
    }

    #[test]
    fn median_depth_of_empty_report_is_none() {
        assert_eq!(MatchReport::default().median_depth(), None);
    }
}
