//! Iterative-deepening paranoid minimax with alpha-beta pruning.
//!
//! Our snake (index 0) is the maximizing player.  All enemy snakes are
//! treated as a single minimizing adversary — for each enemy independently,
//! the move that minimises our evaluation is chosen.  This "paranoid"
//! assumption is conservative but sound and keeps the branching factor
//! manageable: our 4 moves × (≤4 moves per enemy × N enemies) instead of
//! the full 4^(N+1).
//!
//! Time management is handled by iterative deepening: the search starts at
//! depth 1, completes, then tries depth 2, etc.  If the time budget runs
//! out mid-search the previous completed depth's result is returned.

use std::time::Instant;

use crate::evaluation::evaluate;
use crate::heuristic_params::HeuristicParams;
use crate::simulation::{Direction, SimBoard};

/// Result of a completed search.
pub struct SearchResult {
    /// The best move found.
    pub best_move: Direction,
    /// Evaluation score of the best move.
    pub score: i32,
    /// Deepest completed search depth.
    pub depth_reached: u8,
    /// How many nodes were evaluated.
    pub nodes_evaluated: u64,
}

/// Run iterative-deepening search within the given time budget.
///
/// Returns the best move found at the deepest completed depth.
#[must_use]
pub fn search(board: &SimBoard, params: &HeuristicParams, time_budget_ms: u64) -> SearchResult {
    let deadline = Instant::now() + std::time::Duration::from_millis(time_budget_ms);
    let safe_deadline =
        Instant::now() + std::time::Duration::from_millis(time_budget_ms * 60 / 100);

    let our_moves = board.safe_moves(0);

    // Quick eval ordering: sort moves by 1-ply evaluation for better pruning
    let mut ordered_moves: Vec<(Direction, i32)> = our_moves
        .iter()
        .map(|&dir| {
            let mut sim = board.clone();
            let mut moves = vec![dir];
            // Enemies do nothing interesting for ordering — just use Up
            for i in 1..sim.snakes.len() {
                let enemy_moves = sim.safe_moves(i);
                moves.push(enemy_moves[0]);
            }
            sim.apply_moves(&moves);
            let score = evaluate(&sim, params);
            (dir, score)
        })
        .collect();
    ordered_moves.sort_by(|a, b| b.1.cmp(&a.1)); // best first

    let mut best_result = SearchResult {
        best_move: ordered_moves[0].0,
        score: ordered_moves[0].1,
        depth_reached: 0,
        nodes_evaluated: ordered_moves.len() as u64,
    };

    // Iterative deepening
    for depth in 1..=50u8 {
        // Check if we've used more than 60% of our time (safe margin)
        if Instant::now() >= safe_deadline {
            break;
        }

        let mut ctx = SearchContext {
            params,
            deadline,
            nodes: 0,
            timed_out: false,
        };

        let mut best_score = i32::MIN;
        let mut best_move = ordered_moves[0].0;

        for &(dir, _) in &ordered_moves {
            if ctx.timed_out {
                break;
            }

            let mut sim = board.clone();
            // Apply our move, then let enemies respond optimally (minimise)
            let enemy_moves = pick_enemy_moves(&sim, dir, params);
            let mut all_moves = vec![dir];
            all_moves.extend_from_slice(&enemy_moves);
            sim.apply_moves(&all_moves);

            let score = ctx.minimax(&sim, depth - 1, i32::MIN + 1, i32::MAX - 1, false);

            if !ctx.timed_out && score > best_score {
                best_score = score;
                best_move = dir;
            }
        }

        if ctx.timed_out {
            best_result.nodes_evaluated += ctx.nodes;
            break;
        }
        best_result = SearchResult {
            best_move,
            score: best_score,
            depth_reached: depth,
            nodes_evaluated: best_result.nodes_evaluated + ctx.nodes,
        };
    }

    best_result
}

/// For each enemy snake, independently pick the move that minimises our
/// evaluation.  This is the "paranoid" assumption — enemies coordinate
/// against us but we approximate by choosing independently per enemy.
fn pick_enemy_moves(
    board: &SimBoard,
    our_move: Direction,
    params: &HeuristicParams,
) -> Vec<Direction> {
    let mut enemy_moves = Vec::with_capacity(board.snakes.len() - 1);

    for i in 1..board.snakes.len() {
        if !board.snakes[i].alive {
            enemy_moves.push(Direction::Up); // dead snakes don't matter
            continue;
        }

        let safe = board.safe_moves(i);
        let mut worst_score = i32::MAX;
        let mut worst_move = safe[0];

        for &dir in &safe {
            let mut sim = board.clone();
            // Simulate just us + this enemy moving
            let mut moves: Vec<Direction> = vec![our_move];
            for j in 1..sim.snakes.len() {
                if j == i {
                    moves.push(dir);
                } else {
                    let m = sim.safe_moves(j);
                    moves.push(m[0]);
                }
            }
            sim.apply_moves(&moves);
            let score = evaluate(&sim, params);

            if score < worst_score {
                worst_score = score;
                worst_move = dir;
            }
        }

        enemy_moves.push(worst_move);
    }

    enemy_moves
}

struct SearchContext<'a> {
    params: &'a HeuristicParams,
    deadline: Instant,
    nodes: u64,
    timed_out: bool,
}

impl SearchContext<'_> {
    /// Quick 1-ply evaluation to order moves for better alpha-beta pruning.
    fn order_moves(&mut self, board: &SimBoard, moves: &[Direction]) -> Vec<Direction> {
        if moves.len() <= 1 {
            return moves.to_vec();
        }
        let mut scored: Vec<(Direction, i32)> = moves
            .iter()
            .map(|&dir| {
                let mut sim = board.clone();
                let enemy_moves = pick_enemy_moves(&sim, dir, self.params);
                let mut all_moves = vec![dir];
                all_moves.extend_from_slice(&enemy_moves);
                sim.apply_moves(&all_moves);
                self.nodes += 1;
                let score = evaluate(&sim, self.params);
                (dir, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().map(|(d, _)| d).collect()
    }

    fn minimax(
        &mut self,
        board: &SimBoard,
        depth: u8,
        mut alpha: i32,
        mut beta: i32,
        maximizing: bool,
    ) -> i32 {
        self.nodes += 1;

        // Time check every 512 nodes
        if self.nodes.is_multiple_of(512) && Instant::now() >= self.deadline {
            self.timed_out = true;
            return 0;
        }

        // Leaf node or terminal state
        if depth == 0 || board.is_terminal() {
            return evaluate(board, self.params);
        }

        if maximizing {
            // Our turn: try each of our moves, ordered for better pruning
            let our_moves = board.safe_moves(0);
            let ordered = self.order_moves(board, &our_moves);
            let mut best = i32::MIN + 1;

            for dir in &ordered {
                if self.timed_out {
                    return best;
                }

                let mut sim = board.clone();
                let enemy_moves = pick_enemy_moves(&sim, *dir, self.params);
                let mut all_moves = vec![*dir];
                all_moves.extend_from_slice(&enemy_moves);
                sim.apply_moves(&all_moves);

                let score = self.minimax(&sim, depth - 1, alpha, beta, false);
                best = best.max(score);
                alpha = alpha.max(best);
                if alpha >= beta {
                    break; // beta cutoff
                }
            }
            best
        } else {
            // Enemy turn: assume worst case for us.
            // Each "depth" is a full turn (our move + enemy response).
            let our_moves = board.safe_moves(0);
            let ordered = self.order_moves(board, &our_moves);
            let mut worst = i32::MAX - 1;

            for dir in &ordered {
                if self.timed_out {
                    return worst;
                }

                let mut sim = board.clone();
                let enemy_moves = pick_enemy_moves(&sim, *dir, self.params);
                let mut all_moves = vec![*dir];
                all_moves.extend_from_slice(&enemy_moves);
                sim.apply_moves(&all_moves);

                let score = self.minimax(&sim, depth - 1, alpha, beta, true);
                worst = worst.min(score);
                beta = beta.min(worst);
                if alpha >= beta {
                    break; // alpha cutoff
                }
            }
            worst
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game_objects::Coord;
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
    fn test_search_returns_valid_move() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 100);
        let enemy = make_snake(&[(8, 8), (8, 7), (8, 6)], 100);
        let board = make_board(11, 11, vec![us, enemy], vec![Coord::new(3, 3)]);
        let params = default_params();

        let result = search(&board, &params, 100);

        assert!(
            [
                Direction::Up,
                Direction::Down,
                Direction::Left,
                Direction::Right
            ]
            .contains(&result.best_move)
        );
        assert!(result.depth_reached >= 1);
        assert!(result.nodes_evaluated > 0);
    }

    #[test]
    fn test_avoids_wall() {
        // Snake at left edge facing left — should not go left
        let us = make_snake(&[(0, 5), (1, 5), (2, 5)], 100);
        let board = make_board(11, 11, vec![us], vec![]);
        let params = default_params();

        let result = search(&board, &params, 100);

        assert_ne!(result.best_move, Direction::Left);
        assert_ne!(result.best_move, Direction::Right); // that's reversing
    }

    #[test]
    fn test_finds_food_when_hungry() {
        // Snake with low health, food is directly above
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 10);
        let board = make_board(11, 11, vec![us], vec![Coord::new(5, 6)]);
        let params = default_params();

        let result = search(&board, &params, 200);

        assert_eq!(result.best_move, Direction::Up);
    }

    #[test]
    fn test_search_completes_within_budget() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 100);
        let e1 = make_snake(&[(2, 2), (2, 1), (2, 0)], 100);
        let e2 = make_snake(&[(8, 8), (8, 7), (8, 6)], 100);
        let board = make_board(11, 11, vec![us, e1, e2], vec![Coord::new(3, 3)]);
        let params = default_params();

        let start = Instant::now();
        let _result = search(&board, &params, 200);
        let elapsed = start.elapsed();

        // Should complete within roughly the budget (with some overhead)
        assert!(
            elapsed.as_millis() < 400,
            "Search took {}ms, expected < 400ms",
            elapsed.as_millis()
        );
    }
}
