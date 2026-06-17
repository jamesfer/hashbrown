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
use hashbrown_hashjoin::new_map_3::fixed_table::WritableFixedTable;
use hashbrown_hashjoin::new_map_3::group::Group8;

fn make_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(10))
        .measurement_time(Duration::from_secs(30))
        .sample_size(50)
}

fn define_benchmark<P, T, L>(
    c: &mut BenchmarkGroup<WallTime>,
    rng: &mut impl Rng,
    name: &str,
    size: usize,
    mut prepare: P,
    mut write: L)
where
    P: FnMut(&[u64]) -> T,
    L: FnMut(&mut T, &u64),
{
    let element_count = 256usize;
    c.throughput(Throughput::Elements(element_count as u64));
    c.bench_function(name, |bencher| {
        bencher.iter_batched_ref(
            || {
                let hashes = &make_vec(size, || rng.gen_range(1..=u64::MAX));
                let table = prepare(hashes);
                (table, make_vec(element_count, || rng.gen_range(1..=u64::MAX)))
            },
            |(table, keys)| {
                for key in keys {
                    write(table, key);
                }
            },
            BatchSize::SmallInput,
        );
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut rng = StdRng::seed_from_u64(44);

    // Size must be 16 * 2^n
    let size = 16 * 2usize.pow(10);

    for load_ratio in [
        // 0.5,
        0.75,
        // 0.875,
    ] {
        define_all_benchmarks(c, &mut rng, size, load_ratio);
    }
}

fn define_all_benchmarks(
    criterion: &mut Criterion,
    mut rng: &mut StdRng,
    table_capacity: usize,
    load_ratio: f64,
) {
    let value_count = (table_capacity as f64 * load_ratio) as usize;
    let effective_load_ratio = value_count as f64 / table_capacity as f64;
    assert!(effective_load_ratio <= load_ratio, "Effective load ratio {effective_load_ratio} must be lower than testing load ratio {load_ratio}");
    assert!(effective_load_ratio > load_ratio - 0.02, "Effective load ratio {effective_load_ratio} must be greater than testing load ratio {load_ratio} - 0.02");

    let hash_map_requested_capacity = (table_capacity as f64 * load_ratio) as usize;

    let mut group = criterion.benchmark_group(format!("InsertionTime/size:{table_capacity}/load:{load_ratio}"));
    define_benchmark(
        &mut group,
        &mut StdRng::seed_from_u64(44),
        "HashMap",
        value_count,
        |hashes| {
            // Rust's hashmap has an internal load factor of 87.5%
            let mut table = HashMap::with_capacity(hash_map_requested_capacity);
            for (index, hash) in hashes.iter().enumerate() {
                table.insert(*hash, index);
            }
            table
        },
        |table, key| { table.insert(*key, 100); },
    );
    define_benchmark(
        &mut group,
        &mut StdRng::seed_from_u64(44),
        "NewMap3",
        value_count,
        |hashes| {
            let table = WritableFixedTable::<_, Group8>::with_capacity(table_capacity);
            for (index, hash) in hashes.iter().enumerate() {
                table.insert(*hash, index).unwrap();
            }
            table
        },
        |table, key| { table.insert(*key, 100).unwrap(); },
    );
}

criterion_main!(benches);
criterion_group! {
    name = benches;
    config = make_config();
    targets = criterion_benchmark
}
