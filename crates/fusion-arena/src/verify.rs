//! Checks the arena referee against game logs from the official CLI.
//!
//! Logs omit the moves of snakes that die, so each transition passes when some
//! choice of those moves reproduces the next logged position. Food may only be
//! added on top of what the referee left, because spawning is random.

use std::collections::{BTreeMap, VecDeque};

use serde::Deserialize;

use crate::rules::{Game, Move, Point, Snake};

#[derive(Deserialize)]
struct LogPoint {
    x: i8,
    y: i8,
}

impl From<&LogPoint> for Point {
    fn from(point: &LogPoint) -> Self {
        (point.x, point.y)
    }
}

#[derive(Deserialize)]
struct LogSnake {
    id: String,
    health: i32,
    body: Vec<LogPoint>,
}

#[derive(Deserialize)]
struct LogBoard {
    food: Vec<LogPoint>,
    snakes: Vec<LogSnake>,
}

#[derive(Deserialize)]
struct Frame {
    turn: u32,
    board: LogBoard,
}

fn body(snake: &LogSnake) -> VecDeque<Point> {
    snake.body.iter().map(Point::from).collect()
}

fn matches_next(game: &Game, ids: &[&str], next: &LogBoard) -> bool {
    let positions_match = ids.iter().zip(&game.snakes).all(|(id, snake)| {
        match next.snakes.iter().find(|s| s.id == *id) {
            Some(logged) => {
                snake.alive && snake.health == logged.health && snake.body == body(logged)
            }
            None => !snake.alive,
        }
    });
    let next_food: Vec<Point> = next.food.iter().map(Point::from).collect();
    positions_match && game.food.iter().all(|food| next_food.contains(food))
}

fn transition_matches(current: &LogBoard, next: &LogBoard) -> Result<bool, String> {
    let ids: Vec<&str> = current.snakes.iter().map(|s| s.id.as_str()).collect();
    let mut options: Vec<Vec<Move>> = Vec::with_capacity(ids.len());
    for snake in &current.snakes {
        match next.snakes.iter().find(|s| s.id == snake.id) {
            Some(after) => {
                let from = snake.body.first().ok_or("snake has no body")?;
                let to = after.body.first().ok_or("snake has no body")?;
                let mv = Move::between(from.into(), to.into())
                    .ok_or_else(|| format!("snake {} did not move one tile", snake.id))?;
                options.push(vec![mv]);
            }
            None => options.push(Move::ALL.to_vec()),
        }
    }

    let start = Game::from_position(
        current
            .snakes
            .iter()
            .map(|snake| Snake {
                health: snake.health,
                body: body(snake),
                alive: true,
            })
            .collect(),
        current.food.iter().map(Point::from).collect(),
    );

    let mut choice = vec![0_usize; options.len()];
    loop {
        let moves: Vec<Move> = choice
            .iter()
            .zip(&options)
            .map(|(&index, moves)| moves[index])
            .collect();
        let mut game = start.clone();
        game.resolve(&moves);
        if matches_next(&game, &ids, next) {
            return Ok(true);
        }

        let mut slot = 0;
        loop {
            if slot == choice.len() {
                return Ok(false);
            }
            choice[slot] += 1;
            if choice[slot] < options[slot].len() {
                break;
            }
            choice[slot] = 0;
            slot += 1;
        }
    }
}

/// Verifies every turn in one CLI log and returns how many were checked.
///
/// # Errors
/// Returns an error naming the first turn the referee cannot reproduce.
pub fn verify_log(log: &str) -> Result<usize, String> {
    let mut frames = BTreeMap::new();
    for line in log.lines() {
        if let Ok(frame) = serde_json::from_str::<Frame>(line) {
            frames.entry(frame.turn).or_insert(frame.board);
        }
    }

    let mut checked = 0;
    for (turn, board) in &frames {
        let Some(next) = frames.get(&(turn + 1)) else {
            continue;
        };
        if !transition_matches(board, next)? {
            return Err(format!("turn {turn} -> {} does not match", turn + 1));
        }
        checked += 1;
    }
    if checked == 0 {
        return Err("log contains no consecutive turns".to_owned());
    }
    Ok(checked)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(turn: u32, health: i32, head_y: i8) -> String {
        format!(
            r#"{{"turn":{turn},"board":{{"food":[],"snakes":[{{"id":"a","health":{health},"body":[{{"x":5,"y":{head_y}}},{{"x":5,"y":{}}},{{"x":5,"y":{}}}]}}]}}}}"#,
            head_y - 1,
            head_y - 2
        )
    }

    #[test]
    fn accepts_a_correct_transition() {
        let log = format!("{}\n{}", frame(0, 90, 5), frame(1, 89, 6));
        assert_eq!(verify_log(&log), Ok(1));
    }

    #[test]
    fn rejects_wrong_health() {
        let log = format!("{}\n{}", frame(0, 90, 5), frame(1, 70, 6));
        assert!(verify_log(&log).is_err());
    }
}
