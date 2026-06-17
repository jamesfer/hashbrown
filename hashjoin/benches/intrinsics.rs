mod utils;

use crate::utils::input::make_vec;
use criterion::measurement::WallTime;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkGroup, Criterion, Throughput};
use hashbrown_hashjoin::lookup::Lookup;
use hashbrown_hashjoin::new_map_2::fixed_table::FixedTable;
use rand::prelude::SliceRandom;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::future::Future;
use std::time::Duration;
use hashbrown::raw::RawTable;

fn make_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(10))
        .measurement_time(Duration::from_secs(30))
        .sample_size(50)
}

const ITERATIONS: usize = 1_000;
const BATCHES: usize = 256;

fn define_benchmark<const N: usize, R, O>(
    c: &mut BenchmarkGroup<WallTime>,
    rng: &mut impl Rng,
    name: &str,
    mut run: R)
where
    R: FnMut(u8, &[u8; N]) -> O,
{
    c.throughput(Throughput::Elements(ITERATIONS as u64));
    c.bench_function(name, |bencher| {
        bencher.iter_batched_ref(
            || {
                (0..BATCHES).map(|_| {
                    let mut buffer = [0u8; N];
                    rng.fill(&mut buffer[..]);

                    let key = if rng.gen_bool(0.5) {
                        // Take an existing number from the buffer
                        buffer[rng.gen_range(0..N)]
                    } else {
                        // Use a random key
                        rng.gen::<u8>()
                    };
                    (key, buffer)
                }).collect::<Vec<_>>()
            },
            |batches| {
                for (key, buffer) in batches.iter().cycle().take(ITERATIONS) {
                    run(*key, buffer);
                }
            },
            BatchSize::SmallInput,
        );
    });
}

#[cfg(target_arch = "aarch64")]
fn criterion_benchmark(c: &mut Criterion) {
    use hashbrown_hashjoin::new_map_2::intrinsics::u8x16_compare_eq_aarch64_shrink;
    use hashbrown_hashjoin::new_map_2::intrinsics::u8x16_compare_eq_aarch64;
    use hashbrown_hashjoin::new_map_2::intrinsics::u8x8_compare_eq_aarch64;

    let mut rng = StdRng::seed_from_u64(44);

    // Size must be 16 * 2^n

    let mut group = c.benchmark_group("Intrinsics".to_string());
    define_benchmark::<16, _, _>(
        &mut group,
        &mut rng,
        "u8x16",
        |key, buffer| { u8x16_compare_eq_aarch64(key, buffer) },
    );
    define_benchmark::<16, _, _>(
        &mut group,
        &mut rng,
        "u8x16_shrink",
        |key, buffer| { u8x16_compare_eq_aarch64_shrink(key, buffer) },
    );
    define_benchmark::<8, _, _>(
        &mut group,
        &mut rng,
        "u8x8",
        |key, buffer| { u8x8_compare_eq_aarch64(key, buffer) },
    );
}

criterion_main!(benches);
criterion_group! {
    name = benches;
    config = make_config();
    targets = criterion_benchmark
}
