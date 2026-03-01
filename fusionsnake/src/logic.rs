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
use serde::Serialize;

use std::collections::VecDeque;

use crate::game_objects::{Battlesnake, Board, Coord, Game};
use crate::heuristic_params::HeuristicParams;
use crate::responses::{InfoResponse, MoveResponse};

#[derive(Debug, Copy, Clone, PartialEq)]
struct Move {
    direction: Direction,
    coord: Coord,
    safety_score: u8,
    desirability_score: u8,
    space_score: u16,
}

impl Move {
    fn new(direction: Direction, coord: Coord) -> Self {
        Self {
            direction,
            coord,
            safety_score: u8::MAX,
            desirability_score: 0,
            space_score: 0,
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
    fn as_str(self) -> &'static str {
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

    fn choose_best_move_weighted(
        &self,
        safety_weight: u16,
        food_weight: u16,
        space_weight: u16,
    ) -> &'static str {
        // Try to pick from moves with positive safety first
        let best = self.iter().filter(|m| m.safety_score > 0).max_by_key(|m| {
            (u16::from(m.safety_score) * safety_weight)
                + (u16::from(m.desirability_score) * food_weight)
                + (m.space_score * space_weight)
        });

        if let Some(m) = best {
            return m.direction.as_str();
        }

        // All moves have safety 0 — pick the least-bad option
        // (prefer moves with any space or desirability)
        self.iter()
            .max_by_key(|m| {
                (u16::from(m.desirability_score) * food_weight) + (m.space_score * space_weight)
            })
            .map_or("up", |m| m.direction.as_str())
    }
}

/// All observable features captured during a single move decision.
/// Returned alongside the chosen move so callers can log it for ML training.
#[derive(Debug, Clone, Serialize)]
pub struct MoveFeatures {
    // ── turn state ───────────────────────────────────────────────────────────
    pub turn: i32,
    pub health: u32,
    pub length: u32,
    pub head_x: i8,
    pub head_y: i8,
    // ── board ────────────────────────────────────────────────────────────────
    pub board_width: u8,
    pub board_height: u8,
    pub num_snakes: usize,
    pub num_food: usize,
    pub num_hazards: usize,
    pub hazard_damage_per_turn: u32,
    // ── food ─────────────────────────────────────────────────────────────────
    pub target_food_distance: u8,
    pub target_food_contested: bool,
    // ── competition ──────────────────────────────────────────────────────────
    pub max_enemy_length: u32,
    pub min_enemy_length: u32,
    /// `you.length - max_enemy_length`; negative means we are at a disadvantage.
    pub length_advantage: i32,
    // ── per-direction scores ─────────────────────────────────────────────────
    pub up_safety: u8,
    pub up_desirability: u8,
    pub up_space: u16,
    pub down_safety: u8,
    pub down_desirability: u8,
    pub down_space: u16,
    pub left_safety: u8,
    pub left_desirability: u8,
    pub left_space: u16,
    pub right_safety: u8,
    pub right_desirability: u8,
    pub right_space: u16,
    // ── decision ─────────────────────────────────────────────────────────────
    pub chosen_move: &'static str,
    pub safety_weight: u16,
    pub food_weight: u16,
    pub space_weight: u16,
}

// info is called when you create your Battlesnake on play.battlesnake.com
// and controls your Battlesnake's appearance
// TIP: If you open your Battlesnake URL in a browser you should see this data
pub fn info() -> InfoResponse {
    info!("INFO");

    InfoResponse {
        apiversion: "1",
        author: "fusionstreak",
        color: "#BF360C",
        head: "crystal-power",
        tail: "crystal-power",
        version: "0.0.1",
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

/// BFS flood fill from `start` counting reachable open cells.
/// `occupied` should be a flat bool array of size `width * height` marking blocked cells.
#[allow(clippy::cast_sign_loss)]
fn flood_fill(start: Coord, width: i8, height: i8, occupied: &[bool]) -> u16 {
    let w = width.cast_unsigned() as usize;
    let idx =
        |c: Coord| -> usize { c.y.cast_unsigned() as usize * w + c.x.cast_unsigned() as usize };

    if start.x < 0 || start.x >= width || start.y < 0 || start.y >= height {
        return 0;
    }
    if occupied[idx(start)] {
        return 0;
    }

    let total = w * (height.cast_unsigned() as usize);
    let mut visited = vec![false; total];
    visited[idx(start)] = true;

    let mut queue = VecDeque::with_capacity(total);
    queue.push_back(start);
    let mut count: u16 = 0;

    while let Some(cur) = queue.pop_front() {
        count += 1;
        let neighbors = [
            Coord {
                x: cur.x,
                y: cur.y + 1,
            },
            Coord {
                x: cur.x,
                y: cur.y - 1,
            },
            Coord {
                x: cur.x - 1,
                y: cur.y,
            },
            Coord {
                x: cur.x + 1,
                y: cur.y,
            },
        ];
        for n in &neighbors {
            if n.x < 0 || n.x >= width || n.y < 0 || n.y >= height {
                continue;
            }
            let ni = idx(*n);
            if !visited[ni] && !occupied[ni] {
                visited[ni] = true;
                queue.push_back(*n);
            }
        }
    }
    count
}

/// Determine if a snake's tail will move away this turn (i.e. it did NOT just eat).
/// A snake that just ate has a doubled tail: last two body segments are the same coord.
fn tail_will_move(snake: &Battlesnake) -> bool {
    let len = snake.body.len();
    if len < 2 {
        return false;
    }
    // If the last two segments differ, the tail will vacate its current position
    snake.body[len - 1] != snake.body[len - 2]
}

// move is called on every turn and returns your next move
// Valid moves are "up", "down", "left", or "right"
// See https://docs.battlesnake.com/api/example-move for available data
#[allow(clippy::too_many_lines, clippy::cast_sign_loss)]
pub fn get_move(
    game: &Game,
    turn: i32,
    board: &Board,
    you: &Battlesnake,
    params: &HeuristicParams,
) -> (MoveResponse, MoveFeatures) {
    info!("TURN {turn}");

    let w = board.width.cast_signed();
    let h = board.height.cast_signed();

    // Pre-compute enemy length bounds for feature capture and head-to-head logic
    let (max_enemy_length, min_enemy_length) = board
        .snakes
        .iter()
        .filter(|s| s.id != you.id)
        .fold((0u32, u32::MAX), |(mx, mn), s| {
            (mx.max(s.length), mn.min(s.length))
        });
    let min_enemy_length = if min_enemy_length == u32::MAX {
        0
    } else {
        min_enemy_length
    };

    let mut potential_moves: PotentialMoves = PotentialMoves::new(you.head);

    // === Build occupied grid (for collision checks and flood fill) ===
    let grid_size = (board.width as usize) * (board.height as usize);
    let mut occupied = vec![false; grid_size];
    let idx = |c: Coord| -> usize {
        c.y.cast_unsigned() as usize * board.width as usize + c.x.cast_unsigned() as usize
    };

    for snake in &board.snakes {
        let tail_moves = tail_will_move(snake);
        for (i, coord) in snake.body.iter().enumerate() {
            // Skip the tail segment if it will move away this turn
            if tail_moves && i == snake.body.len() - 1 {
                continue;
            }
            if coord.x >= 0 && coord.x < w && coord.y >= 0 && coord.y < h {
                occupied[idx(*coord)] = true;
            }
        }
    }

    // === Determine immediate safety of each move ===
    for mv in potential_moves.iter_mut() {
        // Check if move is out of bounds
        if mv.coord.x < 0 || mv.coord.x >= w || mv.coord.y < 0 || mv.coord.y >= h {
            mv.safety_score = 0;
            continue;
        }

        // Check if move collides with snake bodies (tail-aware)
        if occupied[idx(mv.coord)] {
            mv.safety_score = 0;
        }
    }

    // === Penalize hazard tiles (Royale support) ===
    let hazard_damage = game.ruleset.settings.hazard_damage_per_turn;
    for mv in potential_moves.iter_mut() {
        if mv.safety_score == 0 {
            continue;
        }
        for hazard in &board.hazards {
            if mv.coord == *hazard {
                // If we'd die from the hazard damage, mark as lethal
                if you.health <= hazard_damage {
                    mv.safety_score = 0;
                } else {
                    // Graduated penalty — harsher when health is low
                    let penalty = if you.health < params.hazard_health_threshold {
                        params.hazard_penalty_low_health
                    } else {
                        params.hazard_penalty_high_health
                    };
                    mv.safety_score = mv.safety_score.saturating_sub(penalty);
                }
                break; // coord matches at most one hazard entry per cell
            }
        }
    }

    // === Penalize edge proximity ===
    let edge_dist = params.edge_proximity_distance;
    for mv in potential_moves.iter_mut() {
        if mv.safety_score == 0 {
            continue;
        }
        if mv.coord.x <= edge_dist || mv.coord.x >= w - 1 - edge_dist {
            mv.safety_score = mv
                .safety_score
                .saturating_sub(params.edge_proximity_penalty);
        }
        if mv.coord.y <= edge_dist || mv.coord.y >= h - 1 - edge_dist {
            mv.safety_score = mv
                .safety_score
                .saturating_sub(params.edge_proximity_penalty);
        }
    }

    // === Head-to-head collision avoidance (length-aware) ===
    for mv in potential_moves.iter_mut() {
        if mv.safety_score == 0 {
            continue;
        }
        for snake in &board.snakes {
            if snake.id == you.id {
                continue;
            }
            let enemy_head = snake.head;
            let distance = mv.coord.distance_to(enemy_head);

            if distance <= params.h2h_detection_radius {
                if you.length > snake.length {
                    // We are longer — moving near their head is an opportunity
                    // Give a small desirability bonus for aggressive play
                    if distance == 1 {
                        mv.desirability_score = mv
                            .desirability_score
                            .saturating_add(params.h2h_aggression_bonus);
                    }
                } else {
                    // Equal or shorter — potential death on head-to-head
                    // Distance 1 means our move could directly collide with their next move
                    if distance <= 1 {
                        mv.safety_score = mv.safety_score.saturating_sub(params.h2h_penalty_close);
                    } else {
                        mv.safety_score = mv.safety_score.saturating_sub(params.h2h_penalty_medium);
                    }
                }
            }
        }
    }

    // === Penalize proximity to snake bodies ===
    for mv in potential_moves.iter_mut() {
        if mv.safety_score == 0 {
            continue;
        }
        for snake in &board.snakes {
            for coord in &snake.body {
                let distance = mv.coord.distance_to(*coord);
                if distance == 1 {
                    mv.safety_score = mv
                        .safety_score
                        .saturating_sub(params.body_proximity_penalty);
                }
            }
        }
    }

    // === Smarter food targeting ===
    // Find the best food: prefer food that no longer/equal enemy can reach faster
    let mut best_food: Option<Coord> = None;
    let mut best_food_distance: u8 = u8::MAX;
    let mut target_food_contested = false;

    for food in &board.food {
        let my_dist = you.head.distance_to(*food);

        // Check if a longer-or-equal enemy can reach this food first
        let contested = board.snakes.iter().any(|snake| {
            if snake.id == you.id {
                return false;
            }
            let enemy_dist = snake.head.distance_to(*food);
            // Enemy is at least as long AND can reach food at same time or sooner
            snake.length >= you.length && enemy_dist <= my_dist
        });

        if !contested && my_dist < best_food_distance {
            best_food_distance = my_dist;
            best_food = Some(*food);
        }
    }

    // Fall back to nearest food if all are contested
    if best_food.is_none() {
        target_food_contested = true;
        for food in &board.food {
            let distance = you.head.distance_to(*food);
            if distance < best_food_distance {
                best_food_distance = distance;
                best_food = Some(*food);
            }
        }
    }

    // === Score food desirability ===
    if let Some(target_food) = best_food {
        for mv in potential_moves.iter_mut() {
            if mv.safety_score == 0 {
                continue;
            }
            let distance = mv.coord.distance_to(target_food);
            mv.desirability_score = mv
                .desirability_score
                .saturating_add(params.food_desirability_base.saturating_sub(distance));
        }
    }

    // === Flood fill: score reachable space ===
    for mv in potential_moves.iter_mut() {
        if mv.coord.x < 0 || mv.coord.x >= w || mv.coord.y < 0 || mv.coord.y >= h {
            continue;
        }
        let reachable = flood_fill(mv.coord, w, h, &occupied);
        mv.space_score = reachable;

        // If reachable space is less than our body length, heavily penalize
        // (we'd trap ourselves)
        #[allow(clippy::cast_possible_truncation)]
        let body_len = you.length.min(u32::from(u16::MAX)) as u16;
        if reachable < body_len {
            mv.safety_score = mv
                .safety_score
                .saturating_sub(params.flood_fill_trap_penalty);
        }
    }

    // === Balance weights based on health ===
    let (safety_weight, food_weight, space_weight) =
        if you.health < params.health_threshold_desperate {
            (
                params.weight_desperate_safety,
                params.weight_desperate_food,
                params.weight_desperate_space,
            )
        } else if you.health < params.health_threshold_balanced {
            (
                params.weight_balanced_safety,
                params.weight_balanced_food,
                params.weight_balanced_space,
            )
        } else {
            (
                params.weight_healthy_safety,
                params.weight_healthy_food,
                params.weight_healthy_space,
            )
        };

    let chosen =
        potential_moves.choose_best_move_weighted(safety_weight, food_weight, space_weight);

    info!("MOVE {chosen}");

    #[allow(clippy::cast_possible_wrap)]
    let features = MoveFeatures {
        turn,
        health: you.health,
        length: you.length,
        head_x: you.head.x,
        head_y: you.head.y,
        board_width: board.width,
        board_height: board.height,
        num_snakes: board.snakes.len(),
        num_food: board.food.len(),
        num_hazards: board.hazards.len(),
        hazard_damage_per_turn: game.ruleset.settings.hazard_damage_per_turn,
        target_food_distance: best_food_distance,
        target_food_contested,
        max_enemy_length,
        min_enemy_length,
        length_advantage: you.length as i32 - max_enemy_length as i32,
        up_safety: potential_moves.up.safety_score,
        up_desirability: potential_moves.up.desirability_score,
        up_space: potential_moves.up.space_score,
        down_safety: potential_moves.down.safety_score,
        down_desirability: potential_moves.down.desirability_score,
        down_space: potential_moves.down.space_score,
        left_safety: potential_moves.left.safety_score,
        left_desirability: potential_moves.left.desirability_score,
        left_space: potential_moves.left.space_score,
        right_safety: potential_moves.right.safety_score,
        right_desirability: potential_moves.right.desirability_score,
        right_space: potential_moves.right.space_score,
        chosen_move: chosen,
        safety_weight,
        food_weight,
        space_weight,
    };

    (
        MoveResponse {
            chosen_move: chosen,
        },
        features,
    )
}
