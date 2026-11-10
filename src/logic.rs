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

use crate::game_objects::{Battlesnake, Board, Game};
use crate::responses::InfoResponse;

// info is called when you create your Battlesnake on play.battlesnake.com
// and controls your Battlesnake's appearance
// TIP: If you open your Battlesnake URL in a browser you should see this data
pub fn info() -> InfoResponse {
    info!("INFO");

    InfoResponse {
        apiversion: "1",
        author: "fusionstreak",
        color: "#f54a00",
        head: "pixel-round",
        tail: "mlh-gene",
        version: env!("CARGO_PKG_VERSION"),
    }
}

// start is called when your Battlesnake begins a game
pub fn start(game: &Game, _turn: i32, _board: &Board, _you: &Battlesnake) {
    info!("GAME START {}", game.id);
}

// end is called when your Battlesnake finishes a game
// Returns (won, is_draw) tuple
pub fn end(game: &Game, turn: i32, board: &Board, you: &Battlesnake) -> (bool, bool) {
    // Determine winner
    // Note: board.snakes only contains alive snakes (eliminated snakes are removed)
    let you_alive = board.snakes.iter().any(|s| s.id == you.id);
    let alive_count = board.snakes.len();

    let (won, is_draw) = match alive_count {
        0 => (false, true),      // All died simultaneously (draw)
        1 => (you_alive, false), // One survivor (win if it's you)
        _ => (false, false),     // Multiple survivors (shouldn't happen at game end)
    };

    let winner: Option<String> = if board.snakes.len() == 1 {
        Some(board.snakes[0].name.clone())
    } else {
        None
    };

    info!("GAME OVER {}, Turn {}, Winner: {:?}", game.id, turn, winner);

    (won, is_draw)
}
