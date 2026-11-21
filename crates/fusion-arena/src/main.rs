//! Offline match harness for measuring `FusionSnake` engines.

mod cma;
mod gauntlet;
mod matches;
mod rules;
mod stats;
mod tune;
mod verify;
mod worker;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use gauntlet::{Mode, Runner, Snake, schedule};
use worker::EngineSpec;

#[derive(Parser)]
#[command(about = "Measure FusionSnake engines with reproducible matches")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Play a candidate against running snakes using the official CLI.
    Gauntlet(GauntletArgs),
    /// Play two engine builds against each other with a fixed search budget.
    Match(MatchArgs),
    /// Tune engine parameters through self-play with CMA-ES.
    Tune(tune::TuneArgs),
    /// Check the arena referee against game logs from `battlesnake play -o`.
    VerifyRules {
        /// Log files to check.
        #[arg(required = true)]
        logs: Vec<PathBuf>,
    },
    /// Serve moves over stdin and stdout. Started by `match`.
    #[command(hide = true)]
    Worker(WorkerArgs),
}

#[derive(clap::Args)]
struct MatchArgs {
    /// `fusion-arena` binary for engine A. Defaults to this binary.
    #[arg(long)]
    a_bin: Option<PathBuf>,
    /// `fusion-arena` binary for engine B. Defaults to this binary.
    #[arg(long)]
    b_bin: Option<PathBuf>,
    /// Params JSON for engine A. Defaults to the engine's defaults.
    #[arg(long)]
    a_params: Option<PathBuf>,
    /// Params JSON for engine B. Defaults to the engine's defaults.
    #[arg(long)]
    b_params: Option<PathBuf>,
    /// Board evaluations allowed per move.
    #[arg(long, default_value_t = 20_000)]
    evaluations: u64,
    /// Game formats to play.
    #[arg(long = "mode", value_enum, default_values_t = [Mode::Duel, Mode::Four])]
    modes: Vec<Mode>,
    /// Seeds per format. Each seed is played from every seat.
    #[arg(long, default_value_t = 100)]
    seeds: u32,
    /// First seed.
    #[arg(long, default_value_t = 1)]
    base_seed: u64,
    /// Games to run at once. Defaults to half the CPU cores.
    #[arg(long)]
    threads: Option<usize>,
}

#[derive(clap::Args)]
struct WorkerArgs {
    #[arg(long)]
    params: Option<PathBuf>,
    #[arg(long)]
    evaluations: u64,
}

#[derive(clap::Args)]
struct GauntletArgs {
    /// Snake being measured, as NAME=URL.
    #[arg(long)]
    candidate: Snake,
    /// Opponent as NAME=URL. Repeat for several opponents.
    #[arg(long = "opponent", required = true)]
    opponents: Vec<Snake>,
    /// Game formats to play.
    #[arg(long = "mode", value_enum, default_values_t = [Mode::Duel, Mode::Four])]
    modes: Vec<Mode>,
    /// Seeds per matchup. Each seed is played from every seat.
    #[arg(long, default_value_t = 50)]
    seeds: u32,
    /// First seed.
    #[arg(long, default_value_t = 1)]
    base_seed: u64,
    /// Move timeout in milliseconds.
    #[arg(long, default_value_t = 500)]
    timeout_ms: u32,
    /// Games to run at once. Defaults to one snake per CPU core.
    #[arg(long)]
    concurrency: Option<usize>,
    /// Path to the `battlesnake` CLI.
    #[arg(long, default_value = "battlesnake")]
    cli: PathBuf,
}

fn run_gauntlet(args: &GauntletArgs) {
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    println!(
        "mode      matchup               games   W-D-L            score   95% CI          req-fail failed"
    );

    for &mode in &args.modes {
        let jobs = schedule(
            &args.candidate,
            &args.opponents,
            mode,
            args.seeds,
            args.base_seed,
        );
        let runner = Runner {
            cli: args.cli.clone(),
            timeout_ms: args.timeout_ms,
            concurrency: args
                .concurrency
                .unwrap_or_else(|| (cores / mode.seats()).max(1)),
            work_dir: std::env::temp_dir(),
        };

        for ((mode, matchup), tally) in runner.run(&jobs) {
            let (low, high) = tally.interval();
            println!(
                "{:<9} {:<21} {:<7} {:<16} {:<7.3} [{low:.3}, {high:.3}]  {:<8} {}",
                mode.label(),
                matchup,
                tally.played(),
                format!("{}-{}-{}", tally.wins, tally.draws, tally.losses),
                tally.score(),
                tally.request_failures,
                tally.failed,
            );
        }
    }
}

fn run_match(args: &MatchArgs) -> Result<(), String> {
    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let spec = |binary: &Option<PathBuf>, params: &Option<PathBuf>| EngineSpec {
        binary: binary.clone().unwrap_or_else(|| current.clone()),
        params: params.clone(),
        evaluations: args.evaluations,
    };
    let (a, b) = (
        spec(&args.a_bin, &args.a_params),
        spec(&args.b_bin, &args.b_params),
    );
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let threads = args.threads.unwrap_or((cores / 2).max(1));

    println!(
        "mode      games   W-D-L            A score  95% CI          A depth  failed  seconds"
    );
    for &mode in &args.modes {
        let started = std::time::Instant::now();
        let games = matches::pairings(mode, args.seeds, args.base_seed);
        let report = matches::run(&a, &b, mode, &games, threads);
        let tally = report.tally;
        let (low, high) = tally.interval();
        println!(
            "{:<9} {:<7} {:<16} {:<8.3} [{low:.3}, {high:.3}]  {:<8} {:<7} {:.1}",
            mode.label(),
            tally.played(),
            format!("{}-{}-{}", tally.wins, tally.draws, tally.losses),
            tally.score(),
            report
                .median_depth()
                .map_or_else(|| "-".to_owned(), |d| d.to_string()),
            tally.failed,
            started.elapsed().as_secs_f64(),
        );
    }
    Ok(())
}

fn run_verify(logs: &[PathBuf]) -> Result<(), String> {
    let mut turns = 0;
    let mut mismatches = 0;
    for path in logs {
        let log = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        match verify::verify_log(&log) {
            Ok(checked) => turns += checked,
            Err(error) => {
                mismatches += 1;
                eprintln!("{}: {error}", path.display());
            }
        }
    }
    println!(
        "{} games, {turns} turns matched, {mismatches} games mismatched",
        logs.len()
    );
    if mismatches == 0 {
        Ok(())
    } else {
        Err("referee disagrees with the official rules".to_owned())
    }
}

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Commands::Gauntlet(args) => {
            run_gauntlet(&args);
            Ok(())
        }
        Commands::Match(args) => run_match(&args),
        Commands::Tune(args) => tune::run(&args),
        Commands::VerifyRules { logs } => run_verify(&logs),
        Commands::Worker(args) => worker::serve(args.params.as_deref(), args.evaluations),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}
