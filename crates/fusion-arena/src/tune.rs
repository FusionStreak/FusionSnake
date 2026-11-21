//! Tunes engine parameters through self-play with CMA-ES.

use std::path::{Path, PathBuf};
use std::time::Instant;

use fusion_engine_standard::heuristic_params::HeuristicParams;

use crate::cma::{Cma, default_population};
use crate::gauntlet::Mode;
use crate::matches;
use crate::rules::Rng;
use crate::stats::Tally;
use crate::worker::EngineSpec;

/// `eval_area_weight` is held here, because only the ratios between weights
/// affect move choice. A large value keeps integer rounding from limiting them.
const AREA_ANCHOR: u32 = 100;
/// A past mean joins the opponent pool every this many generations.
const POOL_INTERVAL: u32 = 5;
/// Most past means kept in the opponent pool.
const MAX_PAST_OPPONENTS: usize = 4;

#[derive(Clone, Copy)]
enum Scale {
    /// Multiplicative steps, for weights spanning orders of magnitude.
    Log,
    /// Additive steps, for percentages and health thresholds.
    Linear,
}

struct Gene {
    name: &'static str,
    get: fn(&HeuristicParams) -> u16,
    set: fn(&mut HeuristicParams, u16),
    low: u16,
    high: u16,
    scale: Scale,
    /// Whether the value is a score weight, rescaled with the area anchor.
    weight: bool,
}

macro_rules! gene {
    ($field:ident, $low:expr, $high:expr, $scale:expr, $weight:expr) => {
        Gene {
            name: stringify!($field),
            get: |p| p.$field,
            set: |p, v| p.$field = v,
            low: $low,
            high: $high,
            scale: $scale,
            weight: $weight,
        }
    };
}

const GENES: [Gene; 14] = [
    gene!(eval_health_weight, 1, 5000, Scale::Log, true),
    gene!(eval_food_weight, 1, 5000, Scale::Log, true),
    gene!(eval_length_weight, 1, 5000, Scale::Log, true),
    gene!(eval_aggression_bonus, 1, 5000, Scale::Log, true),
    gene!(eval_trap_penalty, 1, 5000, Scale::Log, true),
    gene!(eval_body_proximity_penalty, 1, 5000, Scale::Log, true),
    gene!(eval_edge_penalty, 1, 5000, Scale::Log, true),
    gene!(eval_h2h_danger_penalty, 1, 5000, Scale::Log, true),
    gene!(eval_desperate_food_mult, 1, 10, Scale::Log, false),
    gene!(eval_healthy_area_mult, 1, 10, Scale::Log, false),
    gene!(eval_healthy_length_mult, 1, 10, Scale::Log, false),
    gene!(eval_food_contest_discount, 0, 100, Scale::Linear, false),
    gene!(health_threshold_desperate, 5, 50, Scale::Linear, false),
    gene!(health_threshold_balanced, 30, 95, Scale::Linear, false),
];

impl Gene {
    fn encode(&self, value: u16) -> f64 {
        match self.scale {
            Scale::Log => f64::from(value.max(1)).ln(),
            // One unit of search space is a quarter of the allowed range.
            Scale::Linear => f64::from(value) * 4.0 / f64::from(self.high - self.low),
        }
    }

    fn decode(&self, value: f64) -> u16 {
        let raw = match self.scale {
            Scale::Log => value.exp(),
            Scale::Linear => value * f64::from(self.high - self.low) / 4.0,
        };
        // Clamped to a u16 range immediately before the cast.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let rounded = raw.round().clamp(f64::from(self.low), f64::from(self.high)) as u16;
        rounded
    }
}

/// Rescales weights so `eval_area_weight` equals [`AREA_ANCHOR`].
fn anchored(start: &HeuristicParams) -> Result<HeuristicParams, String> {
    let area = u32::from(start.eval_area_weight);
    if area == 0 {
        return Err("eval_area_weight must be above 0 to tune".to_owned());
    }
    let mut params = start.clone();
    params.eval_area_weight = u16::try_from(AREA_ANCHOR).map_err(|e| e.to_string())?;
    for gene in GENES.iter().filter(|g| g.weight) {
        let scaled = (u32::from((gene.get)(start)) * AREA_ANCHOR + area / 2) / area;
        let bounded = scaled.clamp(u32::from(gene.low), u32::from(gene.high));
        (gene.set)(
            &mut params,
            u16::try_from(bounded).map_err(|e| e.to_string())?,
        );
    }
    Ok(params)
}

fn encode(params: &HeuristicParams) -> Vec<f64> {
    GENES.iter().map(|g| g.encode((g.get)(params))).collect()
}

fn decode(values: &[f64], base: &HeuristicParams) -> HeuristicParams {
    let mut params = base.clone();
    for (gene, &value) in GENES.iter().zip(values) {
        (gene.set)(&mut params, gene.decode(value));
    }
    if params.health_threshold_balanced <= params.health_threshold_desperate {
        params.health_threshold_balanced = params.health_threshold_desperate + 1;
    }
    params
}

/// Settings for a tuning run.
#[derive(clap::Args)]
pub struct TuneArgs {
    /// Directory for results. Each generation's mean is saved, so a run can
    /// be stopped at any time.
    #[arg(long)]
    out: PathBuf,
    /// Params JSON to start from. Defaults to the engine's defaults.
    #[arg(long)]
    start: Option<PathBuf>,
    /// Generations to run.
    #[arg(long, default_value_t = 40)]
    generations: u32,
    /// Candidates per generation. Defaults to the standard CMA-ES size.
    #[arg(long)]
    population: Option<usize>,
    /// Seeds per candidate and format. Each seed is played from every seat.
    #[arg(long, default_value_t = 4)]
    seeds: u32,
    /// Board evaluations allowed per move.
    #[arg(long, default_value_t = 2_000)]
    evaluations: u64,
    /// Initial search step size.
    #[arg(long, default_value_t = 0.3)]
    sigma: f64,
    /// Seed for the optimizer and game schedule.
    #[arg(long, default_value_t = 1)]
    seed: u64,
    /// Games to run at once. Defaults to half the CPU cores.
    #[arg(long)]
    threads: Option<usize>,
}

/// Combines formats so a result equal to the field scores 0.5 in each.
fn fitness(duel: &Tally, four: &Tally) -> f64 {
    f64::midpoint(duel.score(), four.score() * 2.0)
}

fn write_params(path: &Path, params: &HeuristicParams) -> Result<(), String> {
    let errors = params.validate();
    if !errors.is_empty() {
        return Err(format!("invalid params: {}", errors.join("; ")));
    }
    params.save_to_file(path)
}

fn add(total: &mut Tally, other: Tally) {
    total.wins += other.wins;
    total.draws += other.draws;
    total.losses += other.losses;
    total.failed += other.failed;
}

struct Evaluator {
    binary: PathBuf,
    evaluations: u64,
    seeds: u32,
    threads: usize,
}

impl Evaluator {
    fn spec(&self, params: &Path) -> EngineSpec {
        EngineSpec {
            binary: self.binary.clone(),
            params: Some(params.to_path_buf()),
            evaluations: self.evaluations,
        }
    }

    /// Scores a candidate. Every candidate in a generation gets the same
    /// seeds and opponents, so their scores are directly comparable.
    fn score(&self, candidate: &Path, pool: &[PathBuf], base_seed: u64) -> (f64, u32) {
        let a = self.spec(candidate);
        let (mut duel, mut four) = (Tally::default(), Tally::default());
        for (offset, opponent) in (0_u64..).zip(pool) {
            let b = self.spec(opponent);
            let seed = base_seed + offset * u64::from(self.seeds);
            let games = matches::pairings(Mode::Duel, self.seeds, seed);
            add(
                &mut duel,
                matches::run(&a, &b, Mode::Duel, &games, self.threads).tally,
            );
            let games = matches::pairings(Mode::Four, self.seeds, seed);
            add(
                &mut four,
                matches::run(&a, &b, Mode::Four, &games, self.threads).tally,
            );
        }
        (fitness(&duel, &four), duel.failed + four.failed)
    }
}

/// Runs the tuner and writes `best.json` to the output directory.
///
/// # Errors
/// Returns an error for unreadable params or unwritable output.
pub fn run(args: &TuneArgs) -> Result<(), String> {
    let start = match &args.start {
        Some(path) => HeuristicParams::load_from_file(path)
            .ok_or_else(|| format!("cannot load params from {}", path.display()))?,
        None => HeuristicParams::default(),
    };
    let base = anchored(&start)?;

    std::fs::create_dir_all(&args.out).map_err(|e| e.to_string())?;
    let anchor = args.out.join("start.json");
    write_params(&anchor, &base)?;

    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let evaluator = Evaluator {
        binary: std::env::current_exe().map_err(|e| e.to_string())?,
        evaluations: args.evaluations,
        seeds: args.seeds,
        threads: args.threads.unwrap_or((cores / 2).max(1)),
    };

    let population = args
        .population
        .unwrap_or_else(|| default_population(GENES.len()));
    let mut cma = Cma::new(encode(&base), args.sigma, population);
    let mut rng = Rng::new(args.seed);
    let mut past: Vec<PathBuf> = Vec::new();
    let candidate_path = args.out.join("candidate.json");

    println!("gen  best    mean    sigma   failed  seconds");
    for generation in 1..=args.generations {
        let started = Instant::now();
        let samples = cma.ask(&mut rng);
        let mut pool = vec![anchor.clone()];
        pool.extend(past.iter().cloned());
        // Fresh seeds each generation, so the tuner cannot overfit a fixed set.
        let base_seed = args.seed * 1_000_000 + u64::from(generation) * 10_000;

        let mut scores = Vec::with_capacity(samples.len());
        let mut failed = 0;
        for sample in &samples {
            write_params(&candidate_path, &decode(sample, &base))?;
            let (score, failures) = evaluator.score(&candidate_path, &pool, base_seed);
            scores.push(score);
            failed += failures;
        }
        cma.tell(&samples, &scores);

        let mean_params = decode(&cma.mean, &base);
        let mean_path = args.out.join(format!("mean-{generation:03}.json"));
        write_params(&mean_path, &mean_params)?;
        write_params(&args.out.join("best.json"), &mean_params)?;
        if generation.is_multiple_of(POOL_INTERVAL) {
            past.push(mean_path);
            if past.len() > MAX_PAST_OPPONENTS {
                past.remove(0);
            }
        }

        let best = scores.iter().copied().fold(f64::MIN, f64::max);
        #[allow(clippy::cast_precision_loss)] // A small candidate count converts exactly.
        let average = scores.iter().sum::<f64>() / scores.len() as f64;
        println!(
            "{generation:<4} {best:<7.3} {average:<7.3} {:<7.4} {failed:<7} {:.0}",
            cma.sigma,
            started.elapsed().as_secs_f64()
        );
    }

    let _ = std::fs::remove_file(&candidate_path);
    println!("final params: {}", args.out.join("best.json").display());
    for gene in &GENES {
        let value = (gene.get)(&decode(&cma.mean, &base));
        println!("  {:<30} {value}", gene.name);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ratios_survive_anchoring() {
        let params = anchored(&HeuristicParams::default()).unwrap();
        assert_eq!(params.eval_area_weight, 100);
        assert_eq!(params.eval_food_weight, 50);
        assert_eq!(params.eval_trap_penalty, 500);
        assert_eq!(params.eval_food_contest_discount, 50);
        assert_eq!(params.health_threshold_desperate, 30);
    }

    #[test]
    fn encoding_round_trips_defaults() {
        let base = anchored(&HeuristicParams::default()).unwrap();
        let decoded = decode(&encode(&base), &base);
        for gene in &GENES {
            assert_eq!((gene.get)(&decoded), (gene.get)(&base), "{}", gene.name);
        }
    }

    #[test]
    fn decoded_params_are_always_valid() {
        let base = anchored(&HeuristicParams::default()).unwrap();
        for extreme in [-1000.0, 0.0, 1000.0] {
            let decoded = decode(&vec![extreme; GENES.len()], &base);
            assert!(decoded.validate().is_empty(), "{:?}", decoded.validate());
        }
    }

    #[test]
    fn fitness_of_an_even_result_is_one_half() {
        let duel = Tally {
            wins: 5,
            losses: 5,
            ..Tally::default()
        };
        let four = Tally {
            wins: 5,
            losses: 15,
            ..Tally::default()
        };
        assert!((fitness(&duel, &four) - 0.5).abs() < f64::EPSILON);
    }
}
