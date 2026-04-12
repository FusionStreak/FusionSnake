mod common;

use std::time::Instant;

use criterion::{Criterion, criterion_group, criterion_main};
use fusion_snake::search::search;

fn bench_search_duel_50ms(c: &mut Criterion) {
    let board = common::duel_7x7();
    let params = common::default_params();
    c.bench_function("search/duel_7x7_50ms", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let _ = search(&board, &params, 50);
            }
            start.elapsed()
        });
    });
}

fn bench_search_4snake_50ms(c: &mut Criterion) {
    let board = common::standard_11x11_4snake();
    let params = common::default_params();
    c.bench_function("search/4snake_11x11_50ms", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let _ = search(&board, &params, 50);
            }
            start.elapsed()
        });
    });
}

fn bench_search_duel_150ms(c: &mut Criterion) {
    let board = common::duel_7x7();
    let params = common::default_params();
    let mut group = c.benchmark_group("search_budget");
    group.sample_size(10); // fewer samples since each iteration is ~150ms
    group.bench_function("duel_7x7_150ms", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let _ = search(&board, &params, 150);
            }
            start.elapsed()
        });
    });
    group.finish();
}

fn bench_search_late_game(c: &mut Criterion) {
    let board = common::late_game_11x11();
    let params = common::default_params();
    c.bench_function("search/late_game_11x11_50ms", |b| {
        b.iter_custom(|iters| {
            let start = Instant::now();
            for _ in 0..iters {
                let _ = search(&board, &params, 50);
            }
            start.elapsed()
        });
    });
}

criterion_group!(
    benches,
    bench_search_duel_50ms,
    bench_search_4snake_50ms,
    bench_search_duel_150ms,
    bench_search_late_game
);
criterion_main!(benches);
