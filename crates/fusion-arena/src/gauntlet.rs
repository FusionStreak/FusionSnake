//! Plays real games over HTTP using the official `battlesnake play` CLI.
//!
//! The CLI runs as an external program, so its rules stay authoritative and
//! none of its code is included in this crate.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde::Deserialize;

use crate::stats::Tally;

/// A snake reachable over HTTP.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snake {
    pub name: String,
    pub url: String,
}

impl FromStr for Snake {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (name, url) = value
            .split_once('=')
            .ok_or_else(|| format!("expected NAME=URL, got `{value}`"))?;
        if name.is_empty() || url.is_empty() {
            return Err(format!("expected NAME=URL, got `{value}`"));
        }
        Ok(Self {
            name: name.to_owned(),
            url: url.to_owned(),
        })
    }
}

/// Game format to schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum)]
pub enum Mode {
    /// Candidate against each opponent, one on one.
    Duel,
    /// Candidate against three opponents.
    Four,
}

impl Mode {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Duel => "duel",
            Self::Four => "4-player",
        }
    }

    #[must_use]
    pub fn seats(self) -> usize {
        match self {
            Self::Duel => 2,
            Self::Four => 4,
        }
    }
}

/// One scheduled game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub mode: Mode,
    pub matchup: String,
    pub seed: u64,
    pub seats: Vec<Snake>,
}

/// Name given to the candidate in every game.
const CANDIDATE_SEAT: &str = "candidate";

/// Builds a schedule in which every seed is played from every seat.
#[must_use]
pub fn schedule(
    candidate: &Snake,
    opponents: &[Snake],
    mode: Mode,
    seeds: u32,
    base_seed: u64,
) -> Vec<Job> {
    let mut jobs = Vec::new();
    let candidate = Snake {
        name: CANDIDATE_SEAT.to_owned(),
        url: candidate.url.clone(),
    };

    for index in 0..seeds {
        let seed = base_seed + u64::from(index);
        let tables: Vec<(String, Vec<Snake>)> = match mode {
            Mode::Duel => opponents
                .iter()
                .map(|opponent| (opponent.name.clone(), vec![opponent.clone()]))
                .collect(),
            Mode::Four => {
                let start = index as usize % opponents.len();
                let field = (0..3)
                    .map(|offset| {
                        let opponent = &opponents[(start + offset) % opponents.len()];
                        Snake {
                            name: format!("{}-{offset}", opponent.name),
                            url: opponent.url.clone(),
                        }
                    })
                    .collect();
                vec![("field".to_owned(), field)]
            }
        };

        for (matchup, field) in tables {
            let mut table = Vec::with_capacity(field.len() + 1);
            table.push(candidate.clone());
            table.extend(field);
            for rotation in 0..table.len() {
                let mut seats = table.clone();
                seats.rotate_left(rotation);
                jobs.push(Job {
                    mode,
                    matchup: matchup.clone(),
                    seed,
                    seats,
                });
            }
        }
    }
    jobs
}

/// Final line of a CLI game log.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GameResult {
    winner_name: String,
    is_draw: bool,
}

/// What happened to the candidate in one game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Win,
    Draw,
    Loss,
}

/// Reads the candidate's outcome from a CLI game log.
///
/// # Errors
/// Returns an error when the log has no parseable result line.
pub fn parse_outcome(log: &str) -> Result<Outcome, String> {
    let last = log
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or("game log is empty")?;
    let result: GameResult =
        serde_json::from_str(last).map_err(|e| format!("bad result line: {e}"))?;
    Ok(if result.is_draw {
        Outcome::Draw
    } else if result.winner_name == CANDIDATE_SEAT {
        Outcome::Win
    } else {
        Outcome::Loss
    })
}

/// Settings for running a schedule.
#[derive(Debug, Clone)]
pub struct Runner {
    pub cli: PathBuf,
    pub timeout_ms: u32,
    pub concurrency: usize,
    pub work_dir: PathBuf,
}

impl Runner {
    fn play(&self, job: &Job, index: usize) -> (Result<Outcome, String>, u32) {
        let path = self
            .work_dir
            .join(format!("game-{}-{index}.jsonl", std::process::id()));
        let mut command = Command::new(&self.cli);
        command
            .args([
                "play", "-W", "11", "-H", "11", "-g", "standard", "-m", "standard",
            ])
            .arg("-t")
            .arg(self.timeout_ms.to_string())
            .arg("-r")
            .arg(job.seed.to_string())
            .arg("-o")
            .arg(&path);
        for seat in &job.seats {
            command.arg("-n").arg(&seat.name).arg("-u").arg(&seat.url);
        }

        let result = match command.output() {
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let failures = count_request_failures(&stderr);
                let outcome = if output.status.success() {
                    read_outcome(&path)
                } else {
                    Err(format!("CLI exited with {}", output.status))
                };
                (outcome, failures)
            }
            Err(e) => (Err(format!("could not start CLI: {e}")), 0),
        };
        let _ = std::fs::remove_file(&path);
        result
    }

    /// Runs every job and returns tallies keyed by mode and matchup.
    ///
    /// # Panics
    /// Panics if a worker thread panicked while holding the results lock.
    #[must_use]
    pub fn run(&self, jobs: &[Job]) -> BTreeMap<(Mode, String), Tally> {
        let next = AtomicUsize::new(0);
        let tallies = Mutex::new(BTreeMap::<(Mode, String), Tally>::new());

        std::thread::scope(|scope| {
            for _ in 0..self.concurrency.max(1) {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(job) = jobs.get(index) else { break };
                        let (outcome, failures) = self.play(job, index);
                        let mut guard = tallies.lock().expect("results lock poisoned");
                        let tally = guard.entry((job.mode, job.matchup.clone())).or_default();
                        tally.request_failures += failures;
                        match outcome {
                            Ok(Outcome::Win) => tally.wins += 1,
                            Ok(Outcome::Draw) => tally.draws += 1,
                            Ok(Outcome::Loss) => tally.losses += 1,
                            Err(error) => {
                                tally.failed += 1;
                                eprintln!("game {index} failed: {error}");
                            }
                        }
                    }
                });
            }
        });
        tallies.into_inner().expect("results lock poisoned")
    }
}

fn read_outcome(path: &Path) -> Result<Outcome, String> {
    let log = std::fs::read_to_string(path).map_err(|e| format!("cannot read game log: {e}"))?;
    parse_outcome(&log)
}

fn count_request_failures(stderr: &str) -> u32 {
    let count = stderr
        .lines()
        .filter(|line| line.contains("Request to") && line.contains("failed"))
        .count();
    u32::try_from(count).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snake(name: &str) -> Snake {
        Snake {
            name: name.to_owned(),
            url: format!("http://{name}"),
        }
    }

    #[test]
    fn parses_snake_argument() {
        assert_eq!(
            "a=http://x?y=1".parse::<Snake>().unwrap().url,
            "http://x?y=1"
        );
        assert!("missing".parse::<Snake>().is_err());
        assert!("=http://x".parse::<Snake>().is_err());
    }

    #[test]
    fn duel_plays_each_seed_from_both_seats() {
        let jobs = schedule(&snake("c"), &[snake("a"), snake("b")], Mode::Duel, 3, 10);
        assert_eq!(jobs.len(), 12);
        let first_seats = jobs
            .iter()
            .filter(|j| j.seed == 10 && j.matchup == "a")
            .map(|j| j.seats[0].name.clone())
            .collect::<Vec<_>>();
        assert_eq!(first_seats, ["candidate", "a"]);
    }

    #[test]
    fn four_player_rotates_every_seat_with_unique_names() {
        let jobs = schedule(&snake("c"), &[snake("a")], Mode::Four, 1, 0);
        assert_eq!(jobs.len(), 4);
        for job in &jobs {
            let mut names: Vec<_> = job.seats.iter().map(|s| s.name.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), 4);
        }
        let seats: Vec<_> = jobs
            .iter()
            .map(|j| j.seats.iter().position(|s| s.name == "candidate"))
            .collect();
        assert_eq!(seats, [Some(0), Some(3), Some(2), Some(1)]);
    }

    #[test]
    fn reads_outcomes_from_final_line() {
        let win = "{}\n{\"winnerId\":\"1\",\"winnerName\":\"candidate\",\"isDraw\":false}\n";
        let loss = "{\"winnerId\":\"2\",\"winnerName\":\"a\",\"isDraw\":false}";
        let draw = "{\"winnerId\":\"\",\"winnerName\":\"\",\"isDraw\":true}";
        assert_eq!(parse_outcome(win), Ok(Outcome::Win));
        assert_eq!(parse_outcome(loss), Ok(Outcome::Loss));
        assert_eq!(parse_outcome(draw), Ok(Outcome::Draw));
        assert!(parse_outcome("").is_err());
    }

    #[test]
    fn counts_only_failed_requests() {
        let log = "WARN Request to http://a/move failed\nINFO Turn 1\nWARN Request to http://b/move failed";
        assert_eq!(count_request_failures(log), 2);
    }
}
