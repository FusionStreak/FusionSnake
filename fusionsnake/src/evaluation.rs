//! Static board evaluation for leaf nodes in the search tree.
//!
//! Scores a [`SimBoard`] from our snake's perspective using:
//! - Terminal detection (win/loss)
//! - Voronoi area control (tiles closer to us than any enemy)
//! - Health urgency and food proximity
//! - Length advantage
//! - Aggression bonus near shorter enemy heads

use std::collections::VecDeque;

use crate::game_objects::Coord;
use crate::heuristic_params::HeuristicParams;
use crate::simulation::SimBoard;

/// Evaluate the board position from our snake's perspective.
/// Returns a score where higher = better for us.
///
/// Range: roughly `i32::MIN+1` (dead) to `i32::MAX-1` (won).
pub fn evaluate(board: &SimBoard, params: &HeuristicParams) -> i32 {
    // ── Terminal checks ──────────────────────────────────────────────────
    if !board.we_are_alive() {
        return i32::MIN + 1;
    }
    if board.we_won() {
        return i32::MAX - 1;
    }

    let us = board.us();
    let mut score: i32 = 0;

    // ── Voronoi area control ─────────────────────────────────────────────
    let (our_area, total_area) = voronoi_area(board);
    score += i32::from(params.eval_area_weight) * (our_area * 2 - total_area); // positive when we control > 50%

    // ── Health scoring ───────────────────────────────────────────────────
    // Penalize low health more aggressively
    let health_score = if us.health < 20 {
        us.health * 3 // urgent
    } else {
        us.health
    };
    score += i32::from(params.eval_health_weight) * health_score;

    // ── Food proximity ───────────────────────────────────────────────────
    if !board.food.is_empty() {
        let min_food_dist = board
            .food
            .iter()
            .map(|f| us.head().distance_to(*f))
            .min()
            .unwrap_or(u8::MAX);

        // Invert distance: closer food = higher score
        // Weight more heavily when health is low
        let food_urgency = if us.health < 30 { 3 } else { 1 };
        score += i32::from(params.eval_food_weight)
            * food_urgency
            * i32::from(20_u8.saturating_sub(min_food_dist));
    }

    // ── Length advantage ─────────────────────────────────────────────────
    let max_enemy_len = board
        .snakes
        .iter()
        .skip(1)
        .filter(|s| s.alive)
        .map(SimSnake::length)
        .max()
        .unwrap_or(0);

    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let length_diff = us.length() as i32 - max_enemy_len as i32;
    score += i32::from(params.eval_length_weight) * length_diff;

    // ── Aggression bonus ─────────────────────────────────────────────────
    // Bonus when adjacent to a shorter enemy's head (kill opportunity)
    for enemy in board.snakes.iter().skip(1) {
        if !enemy.alive {
            continue;
        }
        let dist = us.head().distance_to(enemy.head());
        if dist <= 2 && us.length() > enemy.length() {
            score += i32::from(params.eval_aggression_bonus);
        }
    }

    score
}

use crate::simulation::SimSnake;

/// Compute Voronoi area control via simultaneous BFS from all alive snake heads.
///
/// Returns `(our_tiles, total_reachable_tiles)`.
/// Tiles equidistant from multiple snakes are not counted for anyone.
#[allow(clippy::cast_sign_loss)]
fn voronoi_area(board: &SimBoard) -> (i32, i32) {
    const BLOCKED: u8 = 253;
    const CONTESTED: u8 = 254;

    let w = board.width.cast_unsigned() as usize;
    let h = board.height.cast_unsigned() as usize;
    let total = w * h;

    let idx =
        |c: Coord| -> usize { c.y.cast_unsigned() as usize * w + c.x.cast_unsigned() as usize };

    // owner[cell] = snake index that reached it first, or u8::MAX if unvisited.
    // CONTESTED = reached simultaneously by multiple snakes.
    let mut owner = vec![u8::MAX; total];

    // Mark all body segments as blocked
    for snake in &board.snakes {
        if !snake.alive {
            continue;
        }
        for seg in &snake.body {
            if seg.x >= 0 && seg.x < board.width && seg.y >= 0 && seg.y < board.height {
                owner[idx(*seg)] = BLOCKED;
            }
        }
    }

    // Also mark hazards as passable but unowned initially — they're open tiles

    // BFS queues: one per alive snake
    let alive_snakes: Vec<(usize, Coord)> = board
        .snakes
        .iter()
        .enumerate()
        .filter(|(_, s)| s.alive)
        .map(|(i, s)| (i, s.head()))
        .collect();

    let mut queues: Vec<VecDeque<Coord>> = Vec::with_capacity(alive_snakes.len());
    #[allow(clippy::cast_possible_truncation)]
    for &(i, head) in &alive_snakes {
        let mut q = VecDeque::with_capacity(total / alive_snakes.len());
        if head.x >= 0 && head.x < board.width && head.y >= 0 && head.y < board.height {
            let head_idx = idx(head);
            // Head cells are part of the body but the snake "owns" them
            owner[head_idx] = i as u8;
            q.push_back(head);
        }
        queues.push(q);
    }

    // Simultaneous BFS: expand all queues one layer at a time
    loop {
        let mut any_progress = false;

        for (q_idx, queue) in queues.iter_mut().enumerate() {
            let snake_id = alive_snakes[q_idx].0;
            let layer_size = queue.len();
            if layer_size == 0 {
                continue;
            }
            any_progress = true;

            for _ in 0..layer_size {
                let cur = queue.pop_front().unwrap();
                let neighbors = [
                    Coord::new(cur.x, cur.y + 1),
                    Coord::new(cur.x, cur.y - 1),
                    Coord::new(cur.x - 1, cur.y),
                    Coord::new(cur.x + 1, cur.y),
                ];
                for n in &neighbors {
                    if n.x < 0 || n.x >= board.width || n.y < 0 || n.y >= board.height {
                        continue;
                    }
                    let ni = idx(*n);
                    match owner[ni] {
                        u8::MAX => {
                            // Unclaimed — claim it
                            #[allow(clippy::cast_possible_truncation)]
                            {
                                owner[ni] = snake_id as u8;
                            }
                            queue.push_back(*n);
                        }
                        BLOCKED | CONTESTED => {}
                        existing if existing as usize != snake_id => {
                            // Another snake already claimed it at the same BFS depth
                            // → contested
                            owner[ni] = CONTESTED;
                        }
                        _ => {} // already ours
                    }
                }
            }
        }

        if !any_progress {
            break;
        }
    }

    let mut our_count: i32 = 0;
    let mut total_count: i32 = 0;

    for &cell in &owner {
        if cell != u8::MAX && cell != BLOCKED && cell != CONTESTED {
            total_count += 1;
            if cell == 0 {
                our_count += 1;
            }
        }
    }

    (our_count, total_count.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::simulation::SimSnake;

    fn default_params() -> HeuristicParams {
        HeuristicParams::default()
    }

    fn make_snake(body: &[(i8, i8)], health: i32) -> SimSnake {
        SimSnake {
            health,
            body: body.iter().map(|&(x, y)| Coord::new(x, y)).collect(),
            alive: true,
        }
    }

    fn make_board(w: i8, h: i8, snakes: Vec<SimSnake>, food: Vec<Coord>) -> SimBoard {
        SimBoard {
            width: w,
            height: h,
            snakes,
            food,
            hazards: vec![],
            hazard_damage: 0,
        }
    }

    #[test]
    fn test_dead_snake_worst_score() {
        let mut us = make_snake(&[(5, 5), (5, 4)], 0);
        us.alive = false;
        let board = make_board(11, 11, vec![us], vec![]);
        let params = default_params();

        assert_eq!(evaluate(&board, &params), i32::MIN + 1);
    }

    #[test]
    fn test_won_game_best_score() {
        let us = make_snake(&[(5, 5), (5, 4)], 100);
        let board = make_board(11, 11, vec![us], vec![]);
        let params = default_params();

        assert_eq!(evaluate(&board, &params), i32::MAX - 1);
    }

    #[test]
    fn test_more_area_is_better() {
        let params = default_params();

        // Snake in center vs snake in corner — center should have more area
        let us_center = make_snake(&[(5, 5), (5, 4)], 100);
        let enemy_corner = make_snake(&[(0, 0), (1, 0)], 100);
        let board_good = make_board(11, 11, vec![us_center, enemy_corner], vec![]);

        let us_corner = make_snake(&[(0, 0), (1, 0)], 100);
        let enemy_center = make_snake(&[(5, 5), (5, 4)], 100);
        let board_bad = make_board(11, 11, vec![us_corner, enemy_center], vec![]);

        assert!(evaluate(&board_good, &params) > evaluate(&board_bad, &params));
    }

    #[test]
    fn test_food_proximity_helps() {
        let params = default_params();

        let us_near = make_snake(&[(5, 5), (5, 4)], 30);
        let enemy = make_snake(&[(0, 0), (1, 0)], 100);
        let board_near = make_board(11, 11, vec![us_near, enemy], vec![Coord::new(5, 6)]);

        let us_far = make_snake(&[(5, 5), (5, 4)], 30);
        let enemy2 = make_snake(&[(0, 0), (1, 0)], 100);
        let board_far = make_board(11, 11, vec![us_far, enemy2], vec![Coord::new(10, 10)]);

        assert!(evaluate(&board_near, &params) > evaluate(&board_far, &params));
    }
}
