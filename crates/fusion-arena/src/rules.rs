//! Standard rules for offline games on an 11x11 board.
//!
//! This is deliberately separate from the engine's simulator, so a rules bug
//! in the engine shows up as weaker play instead of being shared by the referee.

use std::collections::VecDeque;

pub type Point = (i8, i8);

const SIZE: i8 = 11;
const CENTER: Point = (5, 5);
const MAX_HEALTH: i32 = 100;
const MINIMUM_FOOD: usize = 1;
const FOOD_SPAWN_PERCENT: u64 = 15;
/// Games still running after this many turns are scored as draws.
pub const MAX_TURNS: u32 = 2000;

/// Small seeded generator (`SplitMix64`), so games replay exactly.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        let bound = u64::try_from(bound).expect("bound fits in u64");
        usize::try_from(self.next_u64() % bound).expect("value below a usize bound")
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            items.swap(i, self.below(i + 1));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    Left,
    Right,
}

impl Move {
    pub const ALL: [Self; 4] = [Self::Up, Self::Down, Self::Left, Self::Right];

    /// The move that takes a head from `from` to `to`, if they are adjacent.
    #[must_use]
    pub fn between(from: Point, to: Point) -> Option<Self> {
        Self::ALL.into_iter().find(|mv| mv.apply(from) == to)
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    fn apply(self, (x, y): Point) -> Point {
        match self {
            Self::Up => (x, y + 1),
            Self::Down => (x, y - 1),
            Self::Left => (x - 1, y),
            Self::Right => (x + 1, y),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snake {
    pub health: i32,
    pub body: VecDeque<Point>,
    pub alive: bool,
}

/// Final result of a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finish {
    Winner(usize),
    Draw,
}

#[derive(Debug, Clone)]
pub struct Game {
    pub snakes: Vec<Snake>,
    pub food: Vec<Point>,
    pub turn: u32,
    rng: Rng,
}

fn distance(a: Point, b: Point) -> i16 {
    (i16::from(a.0) - i16::from(b.0)).abs() + (i16::from(a.1) - i16::from(b.1)).abs()
}

impl Game {
    /// Sets up a seeded game: spawns on corners or edge midpoints, one food
    /// diagonal to each snake, and one in the center.
    ///
    /// # Panics
    /// Panics unless `players` is between 1 and 4.
    #[must_use]
    pub fn new(seed: u64, players: usize) -> Self {
        assert!((1..=4).contains(&players), "unsupported player count");
        let mut rng = Rng::new(seed);
        let mut spawns: [Point; 4] = if rng.next_u64() & 1 == 0 {
            [(1, 1), (1, 9), (9, 1), (9, 9)]
        } else {
            [(1, 5), (5, 1), (5, 9), (9, 5)]
        };
        rng.shuffle(&mut spawns);

        let mut food = vec![CENTER];
        let snakes = spawns[..players]
            .iter()
            .map(|&head| {
                let options: Vec<Point> = [(-1, -1), (-1, 1), (1, -1), (1, 1)]
                    .iter()
                    .map(|&(dx, dy)| (head.0 + dx, head.1 + dy))
                    .filter(|&p| {
                        p != CENTER
                            && distance(p, CENTER) <= distance(head, CENTER)
                            && !food.contains(&p)
                    })
                    .collect();
                if !options.is_empty() {
                    food.push(options[rng.below(options.len())]);
                }
                Snake {
                    health: MAX_HEALTH,
                    body: VecDeque::from([head, head, head]),
                    alive: true,
                }
            })
            .collect();

        Self {
            snakes,
            food,
            turn: 0,
            rng,
        }
    }

    /// Builds a game from an existing position.
    #[must_use]
    pub fn from_position(snakes: Vec<Snake>, food: Vec<Point>) -> Self {
        Self {
            snakes,
            food,
            turn: 0,
            rng: Rng::new(0),
        }
    }

    /// Returns the result once one or no snakes remain, or at the turn cap.
    #[must_use]
    pub fn finish(&self) -> Option<Finish> {
        let mut alive = self.snakes.iter().enumerate().filter(|(_, s)| s.alive);
        match (alive.next(), alive.next()) {
            (None, _) => Some(Finish::Draw),
            (Some((index, _)), None) => Some(Finish::Winner(index)),
            _ if self.turn >= MAX_TURNS => Some(Finish::Draw),
            _ => None,
        }
    }

    /// Plays a full turn. `moves` has one entry per snake; moves of eliminated
    /// snakes are ignored.
    pub fn step(&mut self, moves: &[Move]) {
        self.resolve(moves);
        self.spawn_food();
        self.turn += 1;
    }

    /// Applies movement, health, feeding, and eliminations, in official order.
    pub fn resolve(&mut self, moves: &[Move]) {
        for (snake, &mv) in self.snakes.iter_mut().zip(moves) {
            if snake.alive {
                let head = mv.apply(snake.body[0]);
                snake.body.push_front(head);
                snake.body.pop_back();
                snake.health -= 1;
            }
        }

        let snakes = &mut self.snakes;
        self.food.retain(|&food| {
            let mut eaten = false;
            for snake in snakes.iter_mut().filter(|s| s.alive) {
                if snake.body[0] == food {
                    snake.health = MAX_HEALTH;
                    let tail = snake.body[snake.body.len() - 1];
                    snake.body.push_back(tail);
                    eaten = true;
                }
            }
            !eaten
        });

        for snake in self.snakes.iter_mut().filter(|s| s.alive) {
            let (x, y) = snake.body[0];
            if snake.health <= 0 || !(0..SIZE).contains(&x) || !(0..SIZE).contains(&y) {
                snake.alive = false;
            }
        }

        // Collisions are decided first and applied together, so a snake that
        // dies this turn still counts as an obstacle for the others.
        let collided: Vec<usize> = (0..self.snakes.len())
            .filter(|&i| self.snakes[i].alive && self.has_collided(i))
            .collect();
        for index in collided {
            self.snakes[index].alive = false;
        }
    }

    fn has_collided(&self, index: usize) -> bool {
        let snake = &self.snakes[index];
        let head = snake.body[0];
        self.snakes
            .iter()
            .enumerate()
            .filter(|(_, other)| other.alive)
            .any(|(j, other)| {
                other.body.iter().skip(1).any(|&segment| segment == head)
                    || (j != index && other.body[0] == head && snake.body.len() <= other.body.len())
            })
    }

    fn spawn_food(&mut self) {
        let wanted = if self.food.len() < MINIMUM_FOOD {
            MINIMUM_FOOD - self.food.len()
        } else {
            usize::from(self.rng.next_u64() % 100 < FOOD_SPAWN_PERCENT)
        };
        if wanted == 0 {
            return;
        }

        let mut open: Vec<Point> = (0..SIZE)
            .flat_map(|x| (0..SIZE).map(move |y| (x, y)))
            .filter(|p| {
                !self.food.contains(p)
                    && !self
                        .snakes
                        .iter()
                        .filter(|s| s.alive)
                        .any(|s| s.body.contains(p))
            })
            .collect();
        self.rng.shuffle(&mut open);
        self.food.extend(open.into_iter().take(wanted));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(snakes: &[(&[Point], i32)], food: &[Point]) -> Game {
        Game {
            snakes: snakes
                .iter()
                .map(|(body, health)| Snake {
                    health: *health,
                    body: body.iter().copied().collect(),
                    alive: true,
                })
                .collect(),
            food: food.to_vec(),
            turn: 0,
            rng: Rng::new(1),
        }
    }

    #[test]
    fn setup_is_reproducible() {
        let (a, b) = (Game::new(42, 4), Game::new(42, 4));
        assert_eq!(a.snakes, b.snakes);
        assert_eq!(a.food, b.food);
        assert!(a.food.contains(&CENTER));
    }

    #[test]
    fn snake_at_one_health_survives_by_eating() {
        let mut g = game(&[(&[(5, 5), (5, 4), (5, 3)], 1)], &[(5, 6)]);
        g.resolve(&[Move::Up]);
        assert!(g.snakes[0].alive);
        assert_eq!(g.snakes[0].health, 100);
        assert_eq!(g.snakes[0].body.len(), 4);
    }

    #[test]
    fn snake_at_one_health_starves_without_food() {
        let mut g = game(&[(&[(5, 5), (5, 4), (5, 3)], 1)], &[]);
        g.resolve(&[Move::Up]);
        assert!(!g.snakes[0].alive);
    }

    #[test]
    fn longer_snake_wins_head_to_head() {
        let mut g = game(
            &[
                (&[(4, 5), (3, 5), (2, 5), (1, 5)], 90),
                (&[(6, 5), (7, 5), (8, 5)], 90),
            ],
            &[],
        );
        g.resolve(&[Move::Right, Move::Left]);
        assert!(g.snakes[0].alive);
        assert!(!g.snakes[1].alive);
    }

    #[test]
    fn equal_head_to_head_kills_both() {
        let mut g = game(
            &[
                (&[(4, 5), (3, 5), (2, 5)], 90),
                (&[(6, 5), (7, 5), (8, 5)], 90),
            ],
            &[],
        );
        g.resolve(&[Move::Right, Move::Left]);
        assert_eq!(g.finish(), Some(Finish::Draw));
    }

    #[test]
    fn collisions_are_simultaneous() {
        // Snake 0 runs into snake 1's body while snake 1 runs into snake 0's.
        let mut g = game(
            &[
                (&[(4, 5), (4, 4), (4, 3), (4, 2)], 90),
                (&[(5, 4), (5, 5), (5, 6), (5, 7)], 90),
            ],
            &[],
        );
        g.resolve(&[Move::Right, Move::Left]);
        assert!(!g.snakes[0].alive);
        assert!(!g.snakes[1].alive);
    }

    #[test]
    fn food_is_replaced_when_board_is_empty() {
        let mut g = game(&[(&[(5, 5), (5, 4), (5, 3)], 50)], &[]);
        g.step(&[Move::Up]);
        assert_eq!(g.food.len(), 1);
    }
}
