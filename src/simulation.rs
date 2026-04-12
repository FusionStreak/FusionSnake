//! Fast, compact board simulation for tree search.
//!
//! [`SimBoard`] is a self-contained game state that can be cheaply cloned and
//! advanced one turn at a time via [`SimBoard::apply_moves`].  It implements
//! the full Battlesnake Standard ruleset: movement, health decay, food
//! consumption (growth + heal), hazard damage, body/wall collisions, and
//! head-to-head resolution.
//!
//! Our snake is always stored at index 0 for fast access.

use std::collections::VecDeque;

use crate::game_objects::{Battlesnake, Board, Coord};

/// Direction a snake can move.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub const ALL: [Direction; 4] = [
        Direction::Up,
        Direction::Down,
        Direction::Left,
        Direction::Right,
    ];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Left => "left",
            Direction::Right => "right",
        }
    }

    /// Apply this direction to a coordinate, returning the new position.
    #[must_use]
    pub fn apply(self, c: Coord) -> Coord {
        match self {
            Direction::Up => Coord::new(c.x, c.y + 1),
            Direction::Down => Coord::new(c.x, c.y - 1),
            Direction::Left => Coord::new(c.x - 1, c.y),
            Direction::Right => Coord::new(c.x + 1, c.y),
        }
    }

    /// The opposite direction (moving backwards).
    #[allow(dead_code)]
    #[must_use]
    pub fn opposite(self) -> Direction {
        match self {
            Direction::Up => Direction::Down,
            Direction::Down => Direction::Up,
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }
}

/// A single snake in the simulation.
#[derive(Debug, Clone)]
pub struct SimSnake {
    pub health: i32,
    pub body: VecDeque<Coord>,
    pub alive: bool,
}

impl SimSnake {
    #[must_use]
    pub fn head(&self) -> Coord {
        self.body[0]
    }

    #[must_use]
    pub fn length(&self) -> usize {
        self.body.len()
    }

    /// Whether the tail will vacate its position this turn (i.e. the snake
    /// did NOT just eat — detected by the last two segments being different).
    #[must_use]
    pub fn tail_will_move(&self) -> bool {
        let len = self.body.len();
        if len < 2 {
            return false;
        }
        self.body[len - 1] != self.body[len - 2]
    }
}

/// Compact, cloneable game state for tree search.
#[derive(Debug, Clone)]
pub struct SimBoard {
    pub width: i8,
    pub height: i8,
    pub snakes: Vec<SimSnake>,
    pub food: Vec<Coord>,
    pub hazards: Vec<Coord>,
    pub hazard_damage: i32,
}

impl SimBoard {
    /// Build from the Battlesnake API objects.
    /// Our snake (`you_id`) is placed at index 0.
    #[must_use]
    pub fn from_game_state(board: &Board, you_id: &str, hazard_damage: u32) -> Self {
        let mut snakes = Vec::with_capacity(board.snakes.len());

        // Our snake first (index 0)
        for s in &board.snakes {
            if s.id == you_id {
                snakes.push(Self::snake_from_api(s));
                break;
            }
        }

        // Then everyone else
        for s in &board.snakes {
            if s.id != you_id {
                snakes.push(Self::snake_from_api(s));
            }
        }

        Self {
            width: board.width.cast_signed(),
            height: board.height.cast_signed(),
            snakes,
            food: board.food.clone(),
            hazards: board.hazards.clone(),
            #[allow(clippy::cast_possible_wrap)]
            hazard_damage: hazard_damage as i32,
        }
    }

    fn snake_from_api(s: &Battlesnake) -> SimSnake {
        SimSnake {
            #[allow(clippy::cast_possible_wrap)]
            health: s.health as i32,
            body: s.body.iter().copied().collect(),
            alive: true,
        }
    }

    /// Our snake (always index 0).
    #[must_use]
    pub fn us(&self) -> &SimSnake {
        &self.snakes[0]
    }

    /// Whether the game is over from our perspective.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        if !self.snakes[0].alive {
            return true;
        }
        // If we're the only one alive, we've won
        self.snakes.iter().filter(|s| s.alive).count() <= 1
    }

    /// Returns true if our snake is alive.
    #[must_use]
    pub fn we_are_alive(&self) -> bool {
        self.snakes[0].alive
    }

    /// Returns true if we won (alive and sole survivor).
    #[must_use]
    pub fn we_won(&self) -> bool {
        self.snakes[0].alive && self.snakes.iter().filter(|s| s.alive).count() == 1
    }

    /// Number of alive enemy snakes.
    #[allow(dead_code)]
    #[must_use]
    pub fn alive_enemies(&self) -> usize {
        self.snakes.iter().skip(1).filter(|s| s.alive).count()
    }

    /// Apply one full turn of movement and resolve all game rules.
    ///
    /// `moves[0]` is our snake's direction; `moves[i]` for each alive snake
    /// at index `i`.  Dead snakes' moves are ignored.
    ///
    /// Follows the official Battlesnake rule resolution order:
    /// 1. Move all snakes (advance head, maybe remove tail)
    /// 2. Reduce health by 1
    /// 3. Eliminate snakes with health <= 0 (starvation)
    /// 4. Feed snakes on food (restore health, mark for growth)
    /// 5. Apply hazard damage
    /// 6. Eliminate snakes with health <= 0 (hazard death)
    /// 7. Eliminate snakes that moved out of bounds
    /// 8. Eliminate snakes that collided with a body segment
    /// 9. Resolve head-to-head collisions
    pub fn apply_moves(&mut self, moves: &[Direction]) {
        let n = self.snakes.len();
        debug_assert!(moves.len() >= n, "need one move per snake");

        // Track which snakes ate food this turn (grow = don't pop tail)
        let mut ate = vec![false; n];

        // ── Step 1: Move heads ───────────────────────────────────────────
        for (i, snake) in self.snakes.iter_mut().enumerate() {
            if !snake.alive {
                continue;
            }
            let new_head = moves[i].apply(snake.head());
            snake.body.push_front(new_head);
            // Tentatively pop tail (we'll undo if the snake eats)
            snake.body.pop_back();
        }

        // ── Step 2: Reduce health ────────────────────────────────────────
        for snake in &mut self.snakes {
            if snake.alive {
                snake.health -= 1;
            }
        }

        // ── Step 3: Starvation check ─────────────────────────────────────
        for snake in &mut self.snakes {
            if snake.alive && snake.health <= 0 {
                snake.alive = false;
            }
        }

        // ── Step 4: Feed snakes on food ──────────────────────────────────
        self.food.retain(|food| {
            let mut eaten = false;
            for (i, snake) in self.snakes.iter_mut().enumerate() {
                if snake.alive && snake.head() == *food {
                    snake.health = 100;
                    // Grow: duplicate the tail segment
                    if let Some(&tail) = snake.body.back() {
                        snake.body.push_back(tail);
                    }
                    ate[i] = true;
                    eaten = true;
                    // Don't break — multiple snakes can eat at the same position
                    // (they'll be resolved in head-to-head)
                }
            }
            !eaten
        });

        // ── Step 5: Hazard damage ────────────────────────────────────────
        if self.hazard_damage > 0 {
            for snake in &mut self.snakes {
                if !snake.alive {
                    continue;
                }
                let head = snake.head();
                if self.hazards.contains(&head) {
                    snake.health -= self.hazard_damage;
                }
            }
        }

        // ── Step 6: Hazard death check ───────────────────────────────────
        for snake in &mut self.snakes {
            if snake.alive && snake.health <= 0 {
                snake.alive = false;
            }
        }

        // ── Step 7: Out-of-bounds check ──────────────────────────────────
        for snake in &mut self.snakes {
            if !snake.alive {
                continue;
            }
            let head = snake.head();
            if head.x < 0 || head.x >= self.width || head.y < 0 || head.y >= self.height {
                snake.alive = false;
            }
        }

        // ── Step 8: Body collision check ─────────────────────────────────
        // A snake's head moving into any body segment (of any snake) is fatal.
        // We check against the full body *including* the head of other snakes
        // (head-to-head is resolved separately below).
        for i in 0..n {
            if !self.snakes[i].alive {
                continue;
            }
            let head = self.snakes[i].head();
            for other in &self.snakes {
                if !other.alive {
                    continue;
                }
                // Skip checking head-to-head (handled in step 9)
                let start = 1; // skip head (head-to-head handled in step 9)
                if other.body.iter().skip(start).any(|&seg| seg == head) {
                    self.snakes[i].alive = false;
                    break;
                }
            }
        }

        // ── Step 9: Head-to-head resolution ──────────────────────────────
        // If two or more snakes share the same head position, the shorter
        // ones die.  If they are equal length, all die.
        for i in 0..n {
            if !self.snakes[i].alive {
                continue;
            }
            let head_i = self.snakes[i].head();
            for j in (i + 1)..n {
                if !self.snakes[j].alive {
                    continue;
                }
                if self.snakes[j].head() == head_i {
                    let len_i = self.snakes[i].length();
                    let len_j = self.snakes[j].length();
                    match len_i.cmp(&len_j) {
                        std::cmp::Ordering::Less => self.snakes[i].alive = false,
                        std::cmp::Ordering::Greater => self.snakes[j].alive = false,
                        std::cmp::Ordering::Equal => {
                            self.snakes[i].alive = false;
                            self.snakes[j].alive = false;
                        }
                    }
                }
            }
        }
    }

    /// Get non-suicidal moves for a snake (moves that don't immediately go
    /// out of bounds or into a known body segment).  Also excludes moves
    /// that step onto a hazard when the snake would die from the damage.
    /// Returns at least one move (fallback to Up) so search always has
    /// something to try.
    #[must_use]
    pub fn safe_moves(&self, snake_idx: usize) -> Vec<Direction> {
        let snake = &self.snakes[snake_idx];
        if !snake.alive {
            return vec![Direction::Up];
        }
        let head = snake.head();
        let mut moves = Vec::with_capacity(4);

        // Determine the neck direction to avoid reversing
        let neck = if snake.body.len() > 1 {
            Some(snake.body[1])
        } else {
            None
        };

        for &dir in &Direction::ALL {
            let next = dir.apply(head);

            // Don't reverse into our own neck
            if neck == Some(next) {
                continue;
            }

            // Don't go out of bounds
            if next.x < 0 || next.x >= self.width || next.y < 0 || next.y >= self.height {
                continue;
            }

            // Don't go into known body segments (tail-aware)
            let mut blocked = false;
            for s in &self.snakes {
                if !s.alive {
                    continue;
                }
                let tail_moves = s.tail_will_move();
                for (k, seg) in s.body.iter().enumerate() {
                    if tail_moves && k == s.body.len() - 1 {
                        continue;
                    }
                    if *seg == next {
                        blocked = true;
                        break;
                    }
                }
                if blocked {
                    break;
                }
            }

            if blocked {
                continue;
            }

            // Don't step onto a hazard if it would be instantly lethal
            // (health - 1 turn decay - hazard_damage <= 0)
            if self.hazard_damage > 0
                && self.hazards.contains(&next)
                && snake.health - 1 - self.hazard_damage <= 0
            {
                continue;
            }

            moves.push(dir);
        }

        // Always return at least one move
        if moves.is_empty() {
            moves.push(Direction::Up);
        }
        moves
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_snake(body: &[(i8, i8)], health: i32) -> SimSnake {
        SimSnake {
            health,
            body: body.iter().map(|&(x, y)| Coord::new(x, y)).collect(),
            alive: true,
        }
    }

    fn make_board(width: i8, height: i8, snakes: Vec<SimSnake>, food: Vec<Coord>) -> SimBoard {
        SimBoard {
            width,
            height,
            snakes,
            food,
            hazards: vec![],
            hazard_damage: 0,
        }
    }

    #[test]
    fn test_basic_movement() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 100);
        let mut board = make_board(11, 11, vec![us], vec![]);

        board.apply_moves(&[Direction::Up]);

        assert!(board.snakes[0].alive);
        assert_eq!(board.snakes[0].head(), Coord::new(5, 6));
        assert_eq!(board.snakes[0].length(), 3);
        assert_eq!(board.snakes[0].health, 99);
    }

    #[test]
    fn test_food_consumption() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 50);
        let food = vec![Coord::new(5, 6)];
        let mut board = make_board(11, 11, vec![us], food);

        board.apply_moves(&[Direction::Up]);

        assert!(board.snakes[0].alive);
        assert_eq!(board.snakes[0].head(), Coord::new(5, 6));
        assert_eq!(board.snakes[0].length(), 4); // grew
        assert_eq!(board.snakes[0].health, 100); // healed
        assert!(board.food.is_empty()); // food consumed
    }

    #[test]
    fn test_wall_death() {
        let us = make_snake(&[(0, 5), (1, 5), (2, 5)], 100);
        let mut board = make_board(11, 11, vec![us], vec![]);

        board.apply_moves(&[Direction::Left]);

        assert!(!board.snakes[0].alive);
    }

    #[test]
    fn test_starvation() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 1);
        let mut board = make_board(11, 11, vec![us], vec![]);

        board.apply_moves(&[Direction::Up]);

        assert!(!board.snakes[0].alive); // health went to 0
    }

    #[test]
    fn test_head_to_head_shorter_dies() {
        // Two snakes about to collide head-on
        let us = make_snake(&[(4, 5), (3, 5), (2, 5), (1, 5)], 100); // length 4
        let enemy = make_snake(&[(6, 5), (7, 5), (8, 5)], 100); // length 3
        let mut board = make_board(11, 11, vec![us, enemy], vec![]);

        // Both move toward each other to (5, 5)
        board.apply_moves(&[Direction::Right, Direction::Left]);

        assert!(board.snakes[0].alive); // we're longer → survive
        assert!(!board.snakes[1].alive); // enemy shorter → dies
    }

    #[test]
    fn test_head_to_head_equal_both_die() {
        let us = make_snake(&[(4, 5), (3, 5), (2, 5)], 100);
        let enemy = make_snake(&[(6, 5), (7, 5), (8, 5)], 100);
        let mut board = make_board(11, 11, vec![us, enemy], vec![]);

        board.apply_moves(&[Direction::Right, Direction::Left]);

        assert!(!board.snakes[0].alive);
        assert!(!board.snakes[1].alive);
    }

    #[test]
    fn test_body_collision() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 100);
        // Enemy is in a square pattern — moving up puts us into enemy body
        let enemy = make_snake(&[(6, 6), (5, 6), (5, 7), (6, 7)], 100);
        let mut board = make_board(11, 11, vec![us, enemy], vec![]);

        // We move up to (5, 6) — that's the enemy's body[1]
        board.apply_moves(&[Direction::Up, Direction::Right]);

        // After enemy moves Right, their body is [7,6],[6,6],[5,6],[5,7]
        // We moved to (5,6) which is enemy's body[2] → we die
        assert!(!board.snakes[0].alive);
    }

    #[test]
    fn test_hazard_damage() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 50);
        let mut board = make_board(11, 11, vec![us], vec![]);
        board.hazards.push(Coord::new(5, 6));
        board.hazard_damage = 15;

        board.apply_moves(&[Direction::Up]);

        assert!(board.snakes[0].alive);
        // health: 50 - 1 (turn) - 15 (hazard) = 34
        assert_eq!(board.snakes[0].health, 34);
    }

    #[test]
    fn test_terminal_detection() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 100);
        let enemy = make_snake(&[(8, 8), (8, 7), (8, 6)], 100);
        let mut board = make_board(11, 11, vec![us, enemy], vec![]);

        assert!(!board.is_terminal());

        // Kill enemy
        board.snakes[1].alive = false;
        assert!(board.is_terminal());
        assert!(board.we_won());
    }

    #[test]
    fn test_safe_moves_no_reverse() {
        let us = make_snake(&[(5, 5), (5, 4), (5, 3)], 100);
        let board = make_board(11, 11, vec![us], vec![]);

        let moves = board.safe_moves(0);
        // Should not include Down (reverse into neck at 5,4)
        assert!(!moves.contains(&Direction::Down));
        assert!(moves.contains(&Direction::Up));
        assert!(moves.contains(&Direction::Left));
        assert!(moves.contains(&Direction::Right));
    }

    #[test]
    fn test_safe_moves_corner() {
        let us = make_snake(&[(0, 0), (1, 0), (2, 0)], 100);
        let board = make_board(11, 11, vec![us], vec![]);

        let moves = board.safe_moves(0);
        // Can only go Up from (0,0) — Left is OOB, Down is OOB, Right is neck
        assert_eq!(moves, vec![Direction::Up]);
    }
}
