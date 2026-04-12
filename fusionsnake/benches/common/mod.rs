use fusion_snake::game_objects::Coord;
use fusion_snake::heuristic_params::HeuristicParams;
use fusion_snake::simulation::{SimBoard, SimSnake};

pub fn make_snake(body: &[(i8, i8)], health: i32) -> SimSnake {
    SimSnake {
        health,
        body: body.iter().map(|&(x, y)| Coord::new(x, y)).collect(),
        alive: true,
    }
}

pub fn make_board(width: i8, height: i8, snakes: Vec<SimSnake>, food: Vec<Coord>) -> SimBoard {
    SimBoard {
        width,
        height,
        snakes,
        food,
        hazards: Vec::new(),
        hazard_damage: 14,
    }
}

pub fn default_params() -> HeuristicParams {
    HeuristicParams::default()
}

/// 7×7 board with two snakes (duel).
pub fn duel_7x7() -> SimBoard {
    let us = make_snake(&[(3, 3), (3, 2), (3, 1), (2, 1)], 90);
    let enemy = make_snake(&[(5, 5), (5, 4), (5, 3), (4, 3)], 85);
    make_board(
        7,
        7,
        vec![us, enemy],
        vec![Coord::new(1, 5), Coord::new(6, 0)],
    )
}

/// 11×11 board with four snakes and scattered food.
pub fn standard_11x11_4snake() -> SimBoard {
    let us = make_snake(&[(5, 5), (5, 4), (5, 3), (4, 3), (3, 3)], 80);
    let e1 = make_snake(&[(1, 1), (1, 2), (1, 3), (2, 3)], 70);
    let e2 = make_snake(&[(9, 9), (9, 8), (9, 7), (8, 7)], 95);
    let e3 = make_snake(&[(2, 8), (3, 8), (4, 8), (4, 7)], 60);
    make_board(
        11,
        11,
        vec![us, e1, e2, e3],
        vec![
            Coord::new(0, 0),
            Coord::new(10, 10),
            Coord::new(6, 2),
            Coord::new(3, 9),
        ],
    )
}

/// 11×11 late-game board: two long snakes, little food.
pub fn late_game_11x11() -> SimBoard {
    let us = make_snake(
        &[
            (5, 5),
            (5, 4),
            (5, 3),
            (4, 3),
            (3, 3),
            (3, 4),
            (3, 5),
            (3, 6),
            (4, 6),
        ],
        55,
    );
    let enemy = make_snake(
        &[
            (7, 7),
            (7, 6),
            (7, 5),
            (8, 5),
            (9, 5),
            (9, 6),
            (9, 7),
            (8, 7),
        ],
        40,
    );
    make_board(
        11,
        11,
        vec![us, enemy],
        vec![Coord::new(0, 10)],
    )
}
