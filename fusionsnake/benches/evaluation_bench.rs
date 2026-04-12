mod common;

use criterion::{Criterion, criterion_group, criterion_main};
use fusion_snake::evaluation::evaluate;
use fusion_snake::simulation::SimSnake;

fn bench_evaluate_duel(c: &mut Criterion) {
    let board = common::duel_7x7();
    let params = common::default_params();
    c.bench_function("evaluate/duel_7x7", |b| {
        b.iter(|| evaluate(&board, &params));
    });
}

fn bench_evaluate_4snake(c: &mut Criterion) {
    let board = common::standard_11x11_4snake();
    let params = common::default_params();
    c.bench_function("evaluate/4snake_11x11", |b| {
        b.iter(|| evaluate(&board, &params));
    });
}

fn bench_evaluate_late_game(c: &mut Criterion) {
    let board = common::late_game_11x11();
    let params = common::default_params();
    c.bench_function("evaluate/late_game_11x11", |b| {
        b.iter(|| evaluate(&board, &params));
    });
}

fn bench_evaluate_terminal(c: &mut Criterion) {
    let mut board = common::duel_7x7();
    // Kill the enemy so we_won() triggers the short-circuit path
    board.snakes[1] = SimSnake {
        alive: false,
        ..board.snakes[1].clone()
    };
    let params = common::default_params();
    c.bench_function("evaluate/terminal_win", |b| {
        b.iter(|| evaluate(&board, &params));
    });
}

criterion_group!(
    benches,
    bench_evaluate_duel,
    bench_evaluate_4snake,
    bench_evaluate_late_game,
    bench_evaluate_terminal
);
criterion_main!(benches);
