// Welcome to
// __________         __    __  .__                               __
// \______   \_____ _/  |__/  |_|  |   ____   ______ ____ _____  |  | __ ____
//  |    |  _/\__  \\   __\   __\  | _/ __ \ /  ___//    \\__  \ |  |/ // __ \
//  |    |   \ / __ \|  |  |  | |  |_\  ___/ \___ \|   |  \/ __ \|    <\  ___/
//  |________/(______/__|  |__| |____/\_____>______>___|__(______/__|__\\_____>
//
// This file can be a nice home for your Battlesnake logic and helper functions.
//
// To get you started we've included code to prevent your Battlesnake from moving backwards.
// For more info see docs.battlesnake.com

use log::info;
use serde_json::{Value, json};

use crate::game_objects::{Battlesnake, Board, Coord, Game};

#[derive(Debug, Copy, Clone, PartialEq)]
struct Move {
    direction: Direction,
    coord: Coord,
    safety_score: u8,
    desirability_score: u8,
}

impl Move {
    fn new(direction: Direction, coord: Coord) -> Self {
        Self {
            direction,
            coord,
            safety_score: u8::MAX,
            desirability_score: 0,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    fn as_str(&self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Left => "left",
            Direction::Right => "right",
        }
    }
}

#[derive(Debug, Clone)]
struct PotentialMoves {
    pub up: Move,
    pub down: Move,
    pub left: Move,
    pub right: Move,
}

impl PotentialMoves {
    fn new(head: Coord) -> Self {
        Self {
            up: Move::new(
                Direction::Up,
                Coord {
                    x: head.x,
                    y: head.y + 1,
                },
            ),
            down: Move::new(
                Direction::Down,
                Coord {
                    x: head.x,
                    y: head.y - 1,
                },
            ),
            left: Move::new(
                Direction::Left,
                Coord {
                    x: head.x - 1,
                    y: head.y,
                },
            ),
            right: Move::new(
                Direction::Right,
                Coord {
                    x: head.x + 1,
                    y: head.y,
                },
            ),
        }
    }

    fn iter(&self) -> impl Iterator<Item = &Move> {
        [&self.up, &self.down, &self.left, &self.right].into_iter()
    }

    fn iter_mut(&mut self) -> impl Iterator<Item = &mut Move> {
        [
            &mut self.up,
            &mut self.down,
            &mut self.left,
            &mut self.right,
        ]
        .into_iter()
    }

    fn choose_best_move_weighted(&self, safety_weight: u16, food_weight: u16) -> &'static str {
        self.iter()
            .filter(|m| m.safety_score > 0)
            .max_by_key(|m| {
                (m.safety_score as u16 * safety_weight)
                    + (m.desirability_score as u16 * food_weight)
            })
            .map(|m| m.direction.as_str())
            .unwrap_or("up")
    }
}

// info is called when you create your Battlesnake on play.battlesnake.com
// and controls your Battlesnake's appearance
// TIP: If you open your Battlesnake URL in a browser you should see this data
pub fn info() -> Value {
    info!("INFO");

    return json!({
        "apiversion": "1",
        "author": "fusionstreak",
        "color": "#BF360C",
        "head": "mlh-gene",
        "tail": "mlh-gene",
        "version": "0.0.1"
    });
}

// start is called when your Battlesnake begins a game
pub fn start(game: &Game, _turn: &i32, _board: &Board, _you: &Battlesnake) {
    info!("GAME START {}", game.id);
}

// end is called when your Battlesnake finishes a game
pub fn end(game: &Game, turn: &i32, _board: &Board, _you: &Battlesnake) {
    info!("GAME OVER {}, Turn {}", game.id, turn);
}

// move is called on every turn and returns your next move
// Valid moves are "up", "down", "left", or "right"
// See https://docs.battlesnake.com/api/example-move for available data
pub fn get_move(_game: &Game, turn: &i32, board: &Board, you: &Battlesnake) -> Value {
    info!("TURN {}", turn);

    let mut potential_moves: PotentialMoves = PotentialMoves::new(you.head);

    // Determine immediate safety of each move
    for mv in potential_moves.iter_mut() {
        // Check if move is out of bounds
        if mv.coord.x < 0
            || mv.coord.x >= board.width as i8
            || mv.coord.y < 0
            || mv.coord.y >= board.height as i8
        {
            mv.safety_score = 0;
            continue;
        }

        // Check if move collides with other snakes
        for snake in &board.snakes {
            for coord in &snake.body {
                if mv.coord == *coord {
                    mv.safety_score = 0;
                }
            }
        }
    }

    // Penalize edge proximity
    for mv in potential_moves.iter_mut() {
        if mv.safety_score == 0 {
            continue;
        }
        if mv.coord.x <= 1 || mv.coord.x >= (board.width - 2) as i8 {
            mv.safety_score = mv.safety_score.saturating_sub(1);
        }
        if mv.coord.y <= 1 || mv.coord.y >= (board.height - 2) as i8 {
            mv.safety_score = mv.safety_score.saturating_sub(1);
        }
    }

    // Penalize proximity to other snake heads
    for mv in potential_moves.iter_mut() {
        if mv.safety_score == 0 {
            continue;
        }
        for snake in &board.snakes {
            if snake.id == you.id {
                continue;
            }
            let head: Coord = snake.head;
            let distance: u8 = mv.coord.distance_to(&head);
            mv.safety_score = mv
                .safety_score
                .saturating_sub(2 * (board.height.saturating_sub(distance)));
        }
    }

    // Penalize proximity to other snake bodies
    for mv in potential_moves.iter_mut() {
        if mv.safety_score == 0 {
            continue;
        }
        for snake in &board.snakes {
            if snake.id == you.id {
                continue;
            }
            for coord in &snake.body {
                let distance: u8 = mv.coord.distance_to(coord);
                mv.safety_score = mv.safety_score.saturating_sub(board.height - distance);
            }
        }
    }

    // Determine nearest food to snake head
    let mut nearest_food: Coord = Coord { x: 0, y: 0 };
    let mut nearest_food_distance: u8 = u8::MAX;
    for food in &board.food {
        let distance: u8 = you.head.distance_to(food);
        if distance < nearest_food_distance {
            nearest_food = *food;
            nearest_food_distance = distance;
        }
    }

    // Score desirability
    for mv in potential_moves.iter_mut() {
        if mv.safety_score == 0 {
            continue;
        }
        let distance: u8 = mv.coord.distance_to(&nearest_food);
        mv.desirability_score = if distance >= 200 { 0 } else { 200 - distance };
    }

    // Balance weights based on health
    let (safety_weight, food_weight) = if you.health < 30 { (1, 2) } else { (2, 1) };

    let chosen = potential_moves.choose_best_move_weighted(safety_weight, food_weight);

    info!("MOVE {}", chosen);
    json!({ "move": chosen })
}
