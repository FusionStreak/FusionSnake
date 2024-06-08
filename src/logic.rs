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
use rand::seq::SliceRandom;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::game_objects::{Battlesnake, Board, Coord, Game};

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
    let mut is_move_safe: HashMap<_, _> = vec![
        ("up", true),
        ("down", true),
        ("left", true),
        ("right", true),
    ]
    .into_iter()
    .collect();

    let mut potential_moves: Vec<Coord> = vec![
        Coord {
            x: you.head.x,
            y: you.head.y + 1,
        },
        Coord {
            x: you.head.x,
            y: you.head.y - 1,
        },
        Coord {
            x: you.head.x - 1,
            y: you.head.y,
        },
        Coord {
            x: you.head.x + 1,
            y: you.head.y,
        },
    ];

    // Mark out of bounds moves as unsafe
    for (i, coord) in potential_moves.iter().enumerate() {
        if coord.x < 0 || coord.x >= board.width || coord.y < 0 || coord.y >= board.height {
            is_move_safe.insert(
                match i {
                    0 => "up",
                    1 => "down",
                    2 => "left",
                    3 => "right",
                    _ => panic!("Invalid index"),
                },
                false,
            );
        }
    }

    let mut non_safe_coords: Vec<Coord> = vec![];

    // Mark moves that would collide with a snake as unsafe
    let opponents: &Vec<Battlesnake> = &board.snakes;
    for snake in opponents {
        non_safe_coords.extend(snake.body.iter().cloned());
    }

    for coord in non_safe_coords {
        for (i, potential_move) in potential_moves.iter().enumerate() {
            if coord == *potential_move {
                match i {
                    0 => is_move_safe.insert("up", false),
                    1 => is_move_safe.insert("down", false),
                    2 => is_move_safe.insert("left", false),
                    3 => is_move_safe.insert("right", false),
                    _ => panic!("Invalid index"),
                };
            }
        }
    }

    // Are there any safe moves left?
    let safe_moves: Vec<&str> = is_move_safe
        .into_iter()
        .filter(|&(_, v)| v)
        .map(|(k, _)| k)
        .collect::<Vec<_>>();

    // Reset the potential moves
    potential_moves = Vec::new();

    // Add the safe moves to the potential moves
    for safe_move in &safe_moves {
        info!("Safe move: {}", safe_move);
        match safe_move {
            &"up" => potential_moves.push(Coord {
                x: you.head.x,
                y: you.head.y + 1,
            }),
            &"down" => potential_moves.push(Coord {
                x: you.head.x,
                y: you.head.y - 1,
            }),
            &"left" => potential_moves.push(Coord {
                x: you.head.x - 1,
                y: you.head.y,
            }),
            &"right" => potential_moves.push(Coord {
                x: you.head.x + 1,
                y: you.head.y,
            }),
            _ => panic!("Invalid move"),
        };
    }

    // Choose a random move from the safe ones

    // Choose the move that gets us closest to the food
    let food: &Vec<Coord> = &board.food;
    let mut closest_food: &Coord = &food[0];
    let mut closest_distance: i32 = i32::MAX;
    for food_coord in food {
        let distance: i32 = food_coord.distance_to(&you.head);
        if distance < closest_distance {
            closest_distance = distance;
            closest_food = food_coord;
        }
    }

    // Choose the move that gets us closest to the food
    for potential_move in potential_moves.iter() {
        // Calculate the distance to the closest food relative to the potential move
        let dist: i32 = potential_move.distance_to(closest_food);
        // If the potential move is closer to the food than the current closest distance
        if dist < closest_distance {
            if potential_move.x <= closest_food.x {
                // If the potential move is to the left of the food
                return json!({ "move": "right" });
            } else if potential_move.x >= closest_food.x {
                // If the potential move is to the right of the food
                return json!({ "move": "left" });
            } else if potential_move.y <= closest_food.y {
                // If the potential move is below the food
                return json!({ "move": "up" });
            } else if potential_move.y >= closest_food.y {
                // If the potential move is above the food
                return json!({ "move": "down" });
            }
        }
    }

    let chosen: &&str = safe_moves.choose(&mut rand::thread_rng()).unwrap();

    info!("MOVE {}: {}", turn, chosen);
    return json!({ "move": chosen });
}
