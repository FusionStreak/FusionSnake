//! Engine process used by offline matches.
//!
//! Each worker is a built `fusion-arena` binary, so two different engine
//! versions can play each other. Requests and replies are one line each on
//! stdin and stdout.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use fusion_engine_standard::game_objects::Coord;
use fusion_engine_standard::heuristic_params::HeuristicParams;
use fusion_engine_standard::search::{SearchLimit, search_with_limit};
use fusion_engine_standard::simulation::{SimBoard, SimSnake};
use serde::{Deserialize, Serialize};

use crate::rules::{Game, Move, Point};

const BOARD_SIZE: i8 = 11;

#[derive(Serialize, Deserialize)]
struct SnakeView {
    health: i32,
    body: Vec<Point>,
}

/// Board as seen by one snake. The asking snake is always first.
#[derive(Serialize, Deserialize)]
struct MoveRequest {
    food: Vec<Point>,
    snakes: Vec<SnakeView>,
}

impl MoveRequest {
    fn new(game: &Game, seat: usize) -> Self {
        let view = |index: usize| SnakeView {
            health: game.snakes[index].health,
            body: game.snakes[index].body.iter().copied().collect(),
        };
        let mut snakes = vec![view(seat)];
        snakes.extend(
            (0..game.snakes.len())
                .filter(|&i| i != seat && game.snakes[i].alive)
                .map(view),
        );
        Self {
            food: game.food.clone(),
            snakes,
        }
    }

    fn into_board(self) -> SimBoard {
        let coord = |(x, y): Point| Coord::new(x, y);
        SimBoard {
            width: BOARD_SIZE,
            height: BOARD_SIZE,
            snakes: self
                .snakes
                .into_iter()
                .map(|snake| SimSnake {
                    health: snake.health,
                    body: snake.body.into_iter().map(coord).collect::<VecDeque<_>>(),
                    alive: true,
                })
                .collect(),
            food: self.food.into_iter().map(coord).collect(),
            hazards: Vec::new(),
            hazard_damage: 0,
        }
    }
}

/// Serves move requests until stdin closes.
///
/// # Errors
/// Returns an error for unreadable params, malformed requests, or broken pipes.
pub fn serve(params: Option<&Path>, evaluations: u64) -> Result<(), String> {
    let params = match params {
        Some(path) => HeuristicParams::load_from_file(path)
            .ok_or_else(|| format!("cannot load params from {}", path.display()))?,
        None => HeuristicParams::default(),
    };
    let limit = SearchLimit::Evaluations(evaluations);
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let request: MoveRequest = serde_json::from_str(&line).map_err(|e| e.to_string())?;
        let result = search_with_limit(&request.into_board(), &params, limit);
        writeln!(
            stdout,
            "{} {}",
            result.best_move.as_str(),
            result.depth_reached
        )
        .and_then(|()| stdout.flush())
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// How to launch one side of a match.
#[derive(Debug, Clone)]
pub struct EngineSpec {
    pub binary: PathBuf,
    pub params: Option<PathBuf>,
    pub evaluations: u64,
}

/// A running worker process.
pub struct Worker {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Worker {
    /// Starts a worker.
    ///
    /// # Errors
    /// Returns an error when the process cannot be started.
    pub fn spawn(spec: &EngineSpec) -> Result<Self, String> {
        let mut command = Command::new(&spec.binary);
        command
            .arg("worker")
            .arg("--evaluations")
            .arg(spec.evaluations.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        if let Some(params) = &spec.params {
            command.arg("--params").arg(params);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", spec.binary.display()))?;
        let stdin = child.stdin.take().ok_or("worker has no stdin")?;
        let stdout = BufReader::new(child.stdout.take().ok_or("worker has no stdout")?);
        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    /// Asks for the move of the snake in `seat`, returning it with search depth.
    ///
    /// # Errors
    /// Returns an error when the worker exits or sends a malformed reply.
    pub fn choose(&mut self, game: &Game, seat: usize) -> Result<(Move, u8), String> {
        let request =
            serde_json::to_string(&MoveRequest::new(game, seat)).map_err(|e| e.to_string())?;
        writeln!(self.stdin, "{request}")
            .and_then(|()| self.stdin.flush())
            .map_err(|e| format!("worker write failed: {e}"))?;

        let mut reply = String::new();
        let read = self
            .stdout
            .read_line(&mut reply)
            .map_err(|e| format!("worker read failed: {e}"))?;
        if read == 0 {
            return Err("worker exited".to_owned());
        }
        let (mv, depth) = reply
            .trim()
            .split_once(' ')
            .ok_or_else(|| format!("bad worker reply `{}`", reply.trim()))?;
        let mv = Move::parse(mv).ok_or_else(|| format!("bad worker move `{mv}`"))?;
        let depth = depth
            .parse()
            .map_err(|_| format!("bad worker depth `{depth}`"))?;
        Ok((mv, depth))
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
