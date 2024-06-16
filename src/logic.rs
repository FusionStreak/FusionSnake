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
use serde_json::{json, Value};

use crate::game_objects::{Battlesnake, Board, Coord, Game};

#[derive(Debug, Copy, Clone, PartialEq)]
struct Move<'a> {
    direction: &'a str,
    coord: Coord,
    safety_score: u8,
    desirability_score: u8,
}

impl Move<'static> {
    fn new(direction: &str, coord: Coord) -> Move {
        Move {
            direction,
            coord,
            safety_score: u8::MAX,
            desirability_score: 0,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
struct PotentialMoves<'a> {
    up: Move<'a>,
    down: Move<'a>,
    left: Move<'a>,
    right: Move<'a>,
}

impl<'a> IntoIterator for PotentialMoves<'a> {
    type Item = Move<'a>;
    type IntoIter = std::vec::IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        vec![self.up, self.down, self.left, self.right].into_iter()
    }
}

impl PotentialMoves<'static> {
    fn new() -> PotentialMoves<'static> {
        PotentialMoves {
            up: Move::new("up", Coord { x: 0, y: 0 }),
            down: Move::new("down", Coord { x: 0, y: 0 }),
            left: Move::new("left", Coord { x: 0, y: 0 }),
            right: Move::new("right", Coord { x: 0, y: 0 }),
        }
    }

    fn set_move_coord(&mut self, direction: &str, x: i32, y: i32) {
        match direction {
            "up" => self.up.coord = Coord { x, y },
            "down" => self.down.coord = Coord { x, y },
            "left" => self.left.coord = Coord { x, y },
            "right" => self.right.coord = Coord { x, y },
            _ => (),
        }
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

    let mut potential_moves: PotentialMoves = PotentialMoves::new();
    potential_moves.set_move_coord("up", you.head.x, you.head.y + 1);
    potential_moves.set_move_coord("down", you.head.x, you.head.y - 1);
    potential_moves.set_move_coord("left", you.head.x - 1, you.head.y);
    potential_moves.set_move_coord("right", you.head.x + 1, you.head.y);

    // Determine immediate safety of each move
    for mut pmove in potential_moves {
        // Check if move is out of bounds
        if pmove.coord.x < 0
            || pmove.coord.x >= board.width
            || pmove.coord.y < 0
            || pmove.coord.y >= board.height
        {
            pmove.safety_score = 0;
        }
        // Check if move collides with other snakes
        for snake in &board.snakes {
            for coord in &snake.body {
                if pmove.coord == *coord {
                    pmove.safety_score = 0;
                }
            }
        }
    }

    // Check if move is along the edge of the board, if yes reduce safety score by 1
    for mut pmove in potential_moves {
        if pmove.safety_score == 0 {
            continue;
        }
        if pmove.coord.x <= 1 || pmove.coord.x >= -board.width - 2 {
            pmove.safety_score -= 1;
        }
        if pmove.coord.y <= 1 || pmove.coord.y >= board.height - 2 {
            pmove.safety_score -= 1;
        }
    }

    // Check if move is near head of other snakes, if yes reduce safety score by 1
    for mut pmove in potential_moves {
        if pmove.safety_score == 0 {
            continue;
        }
        for snake in &board.snakes {
            if snake.id == you.id {
                continue;
            }
            let head = snake.head;
            if pmove.coord.distance_to(&head) <= 2 {
                pmove.safety_score -= 1;
            }
        }
    }

    // Determine nearest food to snake head
    let mut nearest_food: Coord = Coord { x: 0, y: 0 };
    let mut nearest_food_distance: u8 = u8::MAX;
    for food in &board.food {
        let distance = you.head.distance_to(food);
        if distance < nearest_food_distance {
            nearest_food = *food;
            nearest_food_distance = distance;
        }
    }

    // Determine desirability of each move
    for mut pmove in potential_moves {
        if pmove.safety_score == 0 {
            continue;
        }
        pmove.desirability_score = u8::MAX - pmove.coord.distance_to(&nearest_food);
    }

    // Choose the move with the highest desirability score
    let mut chosen: &str = "up";
    let mut highest_score: u8 = 0;
    for pmove in potential_moves {
        if pmove.safety_score == 0 {
            continue;
        }
        if pmove.desirability_score > highest_score {
            chosen = pmove.direction;
            highest_score = pmove.desirability_score;
        }
    }

    info!("MOVE {}", chosen);
    return json!({ "move": chosen });
}
