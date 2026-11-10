//! Static board evaluation for leaf nodes in the search tree.
//!
//! Scores a [`SimBoard`] from our snake's perspective using:
//! - Terminal detection (win/loss)
//! - Voronoi area control (tiles closer to us than any enemy)
//! - Flood-fill trap detection (reachable space vs body length)
//! - Body proximity penalty (adjacent body segments)
//! - Edge proximity penalty (near board edges)
//! - Health urgency and food proximity (with contestation awareness)
//! - Length advantage
//! - Aggression bonus near shorter enemy heads
//! - Head-to-head danger penalty near equal/longer enemy heads
//! - Health-adaptive weight scaling

use std::collections::VecDeque;

use crate::game_objects::Coord;
use crate::heuristic_params::HeuristicParams;
use crate::simulation::SimBoard;

/// Evaluate the board position from our snake's perspective.
/// Returns a score where higher = better for us.
///
/// Range: roughly `i32::MIN+1` (dead) to `i32::MAX-1` (won).
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::too_many_lines
)]
pub fn evaluate(board: &SimBoard, params: &HeuristicParams) -> i32 {
    // ── Terminal checks ──────────────────────────────────────────────────
    if !board.we_are_alive() {
        return i32::MIN + 1;
    }
    if board.we_won() {
        return i32::MAX - 1;
    }

    let us = board.us();
    let head = us.head();
    let mut score: i32 = 0;

    // ── Health-adaptive multipliers ──────────────────────────────────────
    // Scale evaluation weights based on current health tier.
    let desperate = i32::from(params.health_threshold_desperate);
    let balanced = i32::from(params.health_threshold_balanced);
    let (area_mult, food_mult, length_mult) = if us.health < desperate {
        // Desperate: boost food, reduce area/length focus
        (1, 3, 1)
    } else if us.health < balanced {
        // Balanced: standard weights
        (1, 1, 1)
    } else {
        // Healthy: boost area control and length advantage
        (2, 1, 2)
    };

    // ── Voronoi area control ─────────────────────────────────────────────
    let (our_area, total_area) = voronoi_area(board);
    score += i32::from(params.eval_area_weight) * area_mult * (our_area * 2 - total_area);

    // ── Flood-fill trap detection ────────────────────────────────────────
    let reachable = flood_fill_from_head(board);
    let body_len = us.length() as i32;
    if reachable < body_len {
        // Proportional penalty: the more trapped, the worse
        let shortfall = body_len - reachable;
        score -= i32::from(params.eval_trap_penalty) * shortfall;
    }

    // ── Body proximity penalty ───────────────────────────────────────────
    let mut adjacent_segments: i32 = 0;
    for snake in &board.snakes {
        if !snake.alive {
            continue;
        }
        for seg in &snake.body {
            if head.distance_to(*seg) == 1 {
                adjacent_segments += 1;
            }
        }
    }
    score -= i32::from(params.eval_body_proximity_penalty) * adjacent_segments;

    // ── Edge proximity penalty ───────────────────────────────────────────
    let mut edge_axes: i32 = 0;
    if head.x <= 0 || head.x >= board.width - 1 {
        edge_axes += 1;
    }
    if head.y <= 0 || head.y >= board.height - 1 {
        edge_axes += 1;
    }
    score -= i32::from(params.eval_edge_penalty) * edge_axes;

    // ── Health scoring ───────────────────────────────────────────────────
    let health_score = if us.health < 20 {
        us.health * 3
    } else {
        us.health
    };
    score += i32::from(params.eval_health_weight) * health_score;

    // ── Food proximity (with contestation awareness) ─────────────────────
    if !board.food.is_empty() {
        let mut best_uncontested_dist: Option<u8> = None;
        let mut best_any_dist: u8 = u8::MAX;

        for food in &board.food {
            let my_dist = head.distance_to(*food);
            if my_dist < best_any_dist {
                best_any_dist = my_dist;
            }
            // Check if a longer-or-equal enemy can reach this food at the same
            // time or sooner
            let contested = board.snakes.iter().skip(1).any(|enemy| {
                if !enemy.alive {
                    return false;
                }
                let enemy_dist = enemy.head().distance_to(*food);
                enemy.length() >= us.length() && enemy_dist <= my_dist
            });
            if !contested {
                best_uncontested_dist =
                    Some(best_uncontested_dist.map_or(my_dist, |d| d.min(my_dist)));
            }
        }

        let (food_dist, discount) = if let Some(d) = best_uncontested_dist {
            (d, 100_i32) // full value
        } else {
            (
                best_any_dist,
                100_i32 - i32::from(params.eval_food_contest_discount),
            )
        };

        let food_urgency = if us.health < desperate { 3 } else { 1 };
        let raw_food_score = i32::from(params.eval_food_weight)
            * food_mult
            * food_urgency
            * i32::from(20_u8.saturating_sub(food_dist));
        score += raw_food_score * discount / 100;
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

    let length_diff = us.length() as i32 - max_enemy_len as i32;
    score += i32::from(params.eval_length_weight) * length_mult * length_diff;

    // ── Aggression bonus & head-to-head danger ───────────────────────────
    for enemy in board.snakes.iter().skip(1) {
        if !enemy.alive {
            continue;
        }
        let dist = head.distance_to(enemy.head());
        if dist <= 2 && us.length() > enemy.length() {
            // Kill opportunity
            score += i32::from(params.eval_aggression_bonus);
        }
        if dist <= 1 && us.length() <= enemy.length() {
            // Dangerous head-to-head situation
            score -= i32::from(params.eval_h2h_danger_penalty);
        }
    }

    score
}

use crate::simulation::SimSnake;

/// BFS flood fill from our snake's head, counting reachable open cells.
/// Uses the same occupied grid as simulation (all alive snake bodies, tail-aware).
#[allow(clippy::cast_sign_loss)]
fn flood_fill_from_head(board: &SimBoard) -> i32 {
    let w = board.width.cast_unsigned() as usize;
    let h = board.height.cast_unsigned() as usize;
    let total = w * h;

    let idx =
        |c: Coord| -> usize { c.y.cast_unsigned() as usize * w + c.x.cast_unsigned() as usize };

    let mut occupied = vec![false; total];

    // Mark all body segments as occupied (tail-aware)
    for snake in &board.snakes {
        if !snake.alive {
            continue;
        }
        let tail_moves = snake.tail_will_move();
        for (k, seg) in snake.body.iter().enumerate() {
            if tail_moves && k == snake.body.len() - 1 {
                continue;
            }
            if seg.x >= 0 && seg.x < board.width && seg.y >= 0 && seg.y < board.height {
                occupied[idx(*seg)] = true;
            }
        }
    }

    let start = board.us().head();
    if start.x < 0 || start.x >= board.width || start.y < 0 || start.y >= board.height {
        return 0;
    }

    // The head cell itself is "occupied" by us — unblock it for BFS start.
    let start_idx = idx(start);
    occupied[start_idx] = false;

    let mut visited = vec![false; total];
    visited[start_idx] = true;
    let mut queue = VecDeque::with_capacity(total);
    queue.push_back(start);
    let mut count: i32 = 0;

    while let Some(cur) = queue.pop_front() {
        count += 1;
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
            if !visited[ni] && !occupied[ni] {
                visited[ni] = true;
                queue.push_back(*n);
            }
        }
    }
    count
}

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
                let Some(cur) = queue.pop_front() else {
                    break;
                };
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

    #[test]
    fn test_trapped_snake_penalized() {
        let params = default_params();

        // Snake boxed in by its own body — very limited reachable space
        // Body forms a U-shape leaving only a few open tiles
        let us_trapped = make_snake(
            &[
                (1, 1),
                (1, 2),
                (2, 2),
                (3, 2),
                (3, 1),
                (3, 0),
                (2, 0),
                (1, 0),
            ],
            100,
        );
        let enemy = make_snake(&[(8, 8), (8, 7)], 100);
        let board_trapped = make_board(11, 11, vec![us_trapped, enemy], vec![]);

        // Snake in open space
        let us_free = make_snake(
            &[
                (5, 5),
                (5, 4),
                (5, 3),
                (5, 2),
                (5, 1),
                (5, 0),
                (4, 0),
                (3, 0),
            ],
            100,
        );
        let enemy2 = make_snake(&[(8, 8), (8, 7)], 100);
        let board_free = make_board(11, 11, vec![us_free, enemy2], vec![]);

        assert!(
            evaluate(&board_free, &params) > evaluate(&board_trapped, &params),
            "Free snake should score higher than trapped snake"
        );
    }

    #[test]
    fn test_body_proximity_penalty() {
        let params = default_params();

        // Snake surrounded by enemy body segments
        let us = make_snake(&[(5, 5), (5, 4)], 100);
        let enemy = make_snake(&[(5, 6), (4, 6), (4, 5), (4, 4)], 100);
        let board_crowded = make_board(11, 11, vec![us, enemy], vec![]);

        // Snake far from enemy
        let us2 = make_snake(&[(5, 5), (5, 4)], 100);
        let enemy2 = make_snake(&[(10, 10), (10, 9), (10, 8), (10, 7)], 100);
        let board_open = make_board(11, 11, vec![us2, enemy2], vec![]);

        assert!(
            evaluate(&board_open, &params) > evaluate(&board_crowded, &params),
            "Snake far from enemy body should score higher"
        );
    }

    #[test]
    fn test_edge_penalty() {
        let params = default_params();

        // Snake at corner (two edges)
        let us_corner = make_snake(&[(0, 0), (1, 0)], 100);
        let enemy = make_snake(&[(10, 10), (10, 9)], 100);
        let board_corner = make_board(11, 11, vec![us_corner, enemy], vec![]);

        // Snake in center (no edges)
        let us_center = make_snake(&[(5, 5), (5, 4)], 100);
        let enemy2 = make_snake(&[(10, 10), (10, 9)], 100);
        let board_center = make_board(11, 11, vec![us_center, enemy2], vec![]);

        // Center should score higher due to both edge and area advantages
        assert!(evaluate(&board_center, &params) > evaluate(&board_corner, &params));
    }

    #[test]
    fn test_h2h_danger_penalty() {
        let params = default_params();

        // Our snake adjacent to a longer enemy head — dangerous
        let us_danger = make_snake(&[(5, 5), (5, 4)], 100);
        let enemy_longer = make_snake(&[(5, 6), (5, 7), (5, 8)], 100);
        let board_danger = make_board(11, 11, vec![us_danger, enemy_longer], vec![]);

        // Our snake far from enemy head
        let us_safe = make_snake(&[(5, 5), (5, 4)], 100);
        let enemy_far = make_snake(&[(10, 10), (10, 9), (10, 8)], 100);
        let board_safe = make_board(11, 11, vec![us_safe, enemy_far], vec![]);

        assert!(
            evaluate(&board_safe, &params) > evaluate(&board_danger, &params),
            "Snake near longer enemy head should score lower"
        );
    }

    #[test]
    fn test_uncontested_food_preferred() {
        let params = default_params();

        // Food near us, but a longer enemy is even closer → contested
        let us1 = make_snake(&[(5, 5), (5, 4)], 25);
        let enemy1 = make_snake(&[(3, 6), (3, 7), (3, 8)], 100); // len 3, dist 3 to food
        let board_contested = make_board(
            11,
            11,
            vec![us1, enemy1],
            vec![Coord::new(4, 6)], // dist 2 from us, dist 1 from enemy
        );

        // Food near us, no enemy near it → uncontested
        let us2 = make_snake(&[(5, 5), (5, 4)], 25);
        let enemy2 = make_snake(&[(10, 10), (10, 9), (10, 8)], 100);
        let board_uncontested = make_board(
            11,
            11,
            vec![us2, enemy2],
            vec![Coord::new(5, 6)], // dist 1 from us, far from enemy
        );

        assert!(
            evaluate(&board_uncontested, &params) > evaluate(&board_contested, &params),
            "Uncontested food should score higher"
        );
    }
}
