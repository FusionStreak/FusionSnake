mod common;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fusion_snake::simulation::Direction;

fn bench_apply_moves_duel(c: &mut Criterion) {
    let board = common::duel_7x7();
    c.bench_function("apply_moves/duel_7x7", |b| {
        b.iter_batched(
            || board.clone(),
            |mut sim| {
                sim.apply_moves(&[Direction::Up, Direction::Left]);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_apply_moves_4snake(c: &mut Criterion) {
    let board = common::standard_11x11_4snake();
    c.bench_function("apply_moves/4snake_11x11", |b| {
        b.iter_batched(
            || board.clone(),
            |mut sim| {
                sim.apply_moves(&[
                    Direction::Up,
                    Direction::Left,
                    Direction::Down,
                    Direction::Right,
                ]);
            },
            BatchSize::SmallInput,
        );
    });
}

fn bench_safe_moves(c: &mut Criterion) {
    let board = common::standard_11x11_4snake();
    let mut group = c.benchmark_group("safe_moves");
    for idx in 0..board.snakes.len() {
        group.bench_function(format!("snake_{idx}"), |b| {
            b.iter(|| board.safe_moves(idx));
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_apply_moves_duel,
    bench_apply_moves_4snake,
    bench_safe_moves
);
criterion_main!(benches);
