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

use crate::evaluation::{LOSS_SCORE, WIN_SCORE, evaluate};
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

/// How much work a search may do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchLimit {
    /// Stop on a wall-clock budget in milliseconds. Used for live games.
    Time(u64),
    /// Stop after this many board evaluations. Results do not depend on
    /// machine speed or load, so offline matches are reproducible.
    Evaluations(u64),
}

/// Resolved stopping conditions for one search.
#[derive(Clone, Copy)]
struct Budget {
    deadline: Option<Instant>,
    safe_deadline: Option<Instant>,
    max_evaluations: Option<u64>,
}

impl Budget {
    fn new(limit: SearchLimit) -> Self {
        match limit {
            SearchLimit::Time(ms) => {
                let now = Instant::now();
                Self {
                    deadline: Some(now + std::time::Duration::from_millis(ms)),
                    safe_deadline: Some(now + std::time::Duration::from_millis(ms * 60 / 100)),
                    max_evaluations: None,
                }
            }
            SearchLimit::Evaluations(max) => Self {
                deadline: None,
                safe_deadline: None,
                max_evaluations: Some(max),
            },
        }
    }

    /// Whether another iterative-deepening pass may begin.
    fn can_start_depth(&self, evaluations: u64) -> bool {
        self.safe_deadline.is_none_or(|t| Instant::now() < t)
            && self.max_evaluations.is_none_or(|max| evaluations < max)
    }

    /// Whether the running pass must be abandoned.
    fn exhausted(&self, nodes: u64, evaluations: u64) -> bool {
        self.max_evaluations.is_some_and(|max| evaluations >= max)
            // Reading the clock is comparatively slow, so only do it every 512 nodes.
            || (nodes.is_multiple_of(512) && self.deadline.is_some_and(|t| Instant::now() >= t))
    }
}

/// Run iterative-deepening search within the given time budget.
///
/// Returns the best move found at the deepest completed depth.
#[must_use]
pub fn search(board: &SimBoard, params: &HeuristicParams, time_budget_ms: u64) -> SearchResult {
    search_with_limit(board, params, SearchLimit::Time(time_budget_ms))
}

/// Run iterative-deepening search under an explicit [`SearchLimit`].
///
/// Returns the best move found at the deepest completed depth.
#[must_use]
pub fn search_with_limit(
    board: &SimBoard,
    params: &HeuristicParams,
    limit: SearchLimit,
) -> SearchResult {
    let budget = Budget::new(limit);

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
    ordered_moves.sort_by_key(|b| std::cmp::Reverse(b.1)); // best first

    let mut best_result = SearchResult {
        best_move: ordered_moves[0].0,
        score: ordered_moves[0].1,
        depth_reached: 0,
        nodes_evaluated: ordered_moves.len() as u64,
    };

    let mut evaluations = ordered_moves.len() as u64;

    // Iterative deepening
    for depth in 1..=50u8 {
        // Stop once the safe share of the budget has been used
        if !budget.can_start_depth(evaluations) {
            break;
        }

        let mut ctx = SearchContext {
            params,
            budget,
            nodes: 0,
            evaluations,
            timed_out: false,
        };

        let mut best_score = i32::MIN;
        let mut best_move = ordered_moves[0].0;
        let mut alpha = i32::MIN + 1;

        for &(dir, _) in &ordered_moves {
            if ctx.timed_out {
                break;
            }

            let mut sim = board.clone();
            // Apply our move, then let enemies respond optimally (minimise)
            let enemy_moves = pick_enemy_moves(&sim, dir, params, &mut ctx.evaluations);
            let mut all_moves = vec![dir];
            all_moves.extend_from_slice(&enemy_moves);
            sim.apply_moves(&all_moves);

            let score = ctx.minimax(&sim, depth - 1, 1, alpha, i32::MAX - 1);

            if !ctx.timed_out && score > best_score {
                best_score = score;
                best_move = dir;
                alpha = alpha.max(best_score);
            }
        }

        evaluations = ctx.evaluations;
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
    evaluations: &mut u64,
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
            *evaluations += 1;

            if score < worst_score {
                worst_score = score;
                worst_move = dir;
            }
        }

        enemy_moves.push(worst_move);
    }

    enemy_moves
}

/// Adjusts terminal scores by distance from the root, so the search delays
/// losses and takes wins sooner. Other scores are unchanged.
fn score_at_ply(score: i32, ply: u8) -> i32 {
    match score {
        LOSS_SCORE => LOSS_SCORE + i32::from(ply),
        WIN_SCORE => WIN_SCORE - i32::from(ply),
        other => other,
    }
}

struct SearchContext<'a> {
    params: &'a HeuristicParams,
    budget: Budget,
    nodes: u64,
    evaluations: u64,
    timed_out: bool,
}

impl SearchContext<'_> {
    fn minimax(&mut self, board: &SimBoard, depth: u8, ply: u8, mut alpha: i32, beta: i32) -> i32 {
        self.nodes += 1;

        if self.budget.exhausted(self.nodes, self.evaluations) {
            self.timed_out = true;
            return 0;
        }

        // Leaf node or terminal state
        if depth == 0 || board.is_terminal() {
            self.evaluations += 1;
            return score_at_ply(evaluate(board, self.params), ply);
        }

        // Our turn: try each of our moves. Enemies respond adversarially via
        // pick_enemy_moves at every ply — no separate minimizing ply needed.
        let our_moves = board.safe_moves(0);
        let mut best = i32::MIN + 1;

        for dir in &our_moves {
            if self.timed_out {
                return best;
            }

            let mut sim = board.clone();
            let enemy_moves = pick_enemy_moves(&sim, *dir, self.params, &mut self.evaluations);
            let mut all_moves = vec![*dir];
            all_moves.extend_from_slice(&enemy_moves);
            sim.apply_moves(&all_moves);

            let score = self.minimax(&sim, depth - 1, ply + 1, alpha, beta);
            best = best.max(score);
            alpha = alpha.max(best);
            if alpha >= beta {
                break;
            }
        }
        best
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
    fn test_later_loss_and_sooner_win_score_higher() {
        assert!(score_at_ply(LOSS_SCORE, 5) > score_at_ply(LOSS_SCORE, 1));
        assert!(score_at_ply(WIN_SCORE, 1) > score_at_ply(WIN_SCORE, 5));
        assert_eq!(score_at_ply(1234, 5), 1234);
    }

    #[test]
    fn test_forced_loss_prefers_longest_survival() {
        // Lethal hazards wall off two dead ends. Right is one tile deep and
        // nearer the food, so it is tried first. Left is two tiles deep.
        let us = make_snake(&[(3, 0), (3, 1), (3, 2)], 100);
        let enemy = make_snake(&[(8, 8), (8, 9), (8, 10)], 100);
        let mut board = make_board(11, 11, vec![us, enemy], vec![Coord::new(5, 1)]);
        board.hazards = [(0, 0), (1, 1), (2, 1), (4, 1), (5, 0)]
            .iter()
            .map(|&(x, y)| Coord::new(x, y))
            .collect();
        board.hazard_damage = 200;

        let result = search_with_limit(&board, &default_params(), SearchLimit::Evaluations(5_000));

        assert_eq!(result.best_move, Direction::Left);
        assert_eq!(result.score, LOSS_SCORE + 3);
    }

    #[test]
    fn test_evaluation_limit_is_deterministic() {
        let make = || {
            let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 100);
            let e1 = make_snake(&[(2, 2), (2, 1), (2, 0)], 100);
            let e2 = make_snake(&[(8, 8), (8, 7), (8, 6)], 100);
            make_board(11, 11, vec![us, e1, e2], vec![Coord::new(3, 3)])
        };
        let params = default_params();
        let limit = SearchLimit::Evaluations(5_000);

        let first = search_with_limit(&make(), &params, limit);
        let second = search_with_limit(&make(), &params, limit);

        assert_eq!(first.best_move, second.best_move);
        assert_eq!(first.score, second.score);
        assert_eq!(first.depth_reached, second.depth_reached);
        assert_eq!(first.nodes_evaluated, second.nodes_evaluated);
    }

    #[test]
    fn test_larger_evaluation_limit_searches_deeper() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 100);
        let enemy = make_snake(&[(8, 8), (8, 7), (8, 6)], 100);
        let board = make_board(11, 11, vec![us, enemy], vec![Coord::new(3, 3)]);
        let params = default_params();

        let small = search_with_limit(&board, &params, SearchLimit::Evaluations(100));
        let large = search_with_limit(&board, &params, SearchLimit::Evaluations(20_000));

        assert!(large.depth_reached > small.depth_reached);
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
