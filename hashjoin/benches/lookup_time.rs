mod utils;

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkGroup, Criterion, Throughput};
use criterion::measurement::WallTime;
use rand::prelude::SliceRandom;
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinSet;
use hashbrown_hashjoin::builder::attempt3::ConcurrentBuilder;
use hashbrown_hashjoin::dash_map_builder::DashMapBuilder;
use hashbrown_hashjoin::leapfrog_builder::LeapfrogBuilder;
use hashbrown_hashjoin::lookup::Lookup;
use hashbrown_hashjoin::mutex_map_builder::MutexMapBuilder;
use crate::utils::input::{limited_random, make_input};

// Number of power cores on an Apple M1 max
const PARALLELISM: usize = 8;

fn make_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(10))
        .measurement_time(Duration::from_secs(30))
        .sample_size(50)
}

fn define_benchmark<P, I, R, F, L>(
    c: &mut BenchmarkGroup<WallTime>,
    runtime: &Runtime,
    name: &str,
    mut prepare: P,
    mut build: R,
    all_hashes: &Vec<u64>,
    lookup_count: usize)
where
    P: FnMut() -> Vec<I>,
    R: FnMut(usize, I) -> F,
    F: Future<Output = L> + Send + 'static,
    L: Lookup + Send + 'static,
{
    c.throughput(Throughput::Elements(lookup_count as u64));
    c.bench_function(name, |bencher| {
        // Build the lookup table
        let lookup = runtime.block_on(async {
            let inputs = prepare();
            let mut join_set = JoinSet::new();
            for (thread_index, input) in inputs.into_iter().enumerate() {
                join_set.spawn(build(thread_index, input));
            }
            let mut lookups = join_set.join_all().await;
            lookups.pop().expect("No lookup implementations were generated")
        });

        bencher.iter_batched_ref(
            || all_hashes.choose_multiple(&mut rand::thread_rng(), lookup_count)
                .copied()
                .collect::<Vec<_>>(),
            |keys_to_lookup| {
                keys_to_lookup.iter().map(|hash| lookup.get(*hash)).collect::<Vec<_>>()
            },
            BatchSize::SmallInput,
        );
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    let rt = Builder::new_multi_thread()
        .worker_threads(PARALLELISM)
        .build()
        .unwrap();

    let inputs = vec![
        ("Random", make_input(PARALLELISM, 10, 1024, rand::random)),
        ("Random", make_input(PARALLELISM, 64, 8192, rand::random)),
        ("Choice1024", make_input(PARALLELISM, 10, 256, limited_random(1024))),
    ];
    for (name, input) in inputs {
        let total_size = input.iter().flatten().flatten().count();
        let all_keys = input.iter().flatten().flatten().map(|x| *x).collect::<Vec<_>>();
        let lookup_count = 16384;
        let mut group = c.benchmark_group(format!("LookupThroughput/{name}/Size{total_size}"));
        define_benchmark(
            &mut group,
            &rt,
            "MutexMap",
            || {
                let builder = Arc::new(MutexMapBuilder::new(PARALLELISM));
                input.iter().map(|batches| (Arc::clone(&builder), batches.clone())).collect()
            },
            |thread_index, (builder, batches)| async move { builder.run(thread_index, batches).await },
            &all_keys,
            lookup_count,
        );
        define_benchmark(
            &mut group,
            &rt,
            "DashMap",
            || {
                let builder = Arc::new(DashMapBuilder::new(PARALLELISM));
                input.iter().map(|batches| (Arc::clone(&builder), batches.clone())).collect()
            },
            |thread_index, (builder, batches)| async move { builder.run(thread_index, batches).await },
            &all_keys,
            lookup_count,
        );
        // define_benchmark(
        //     &mut group,
        //     &rt,
        //     "LeapFrog",
        //     || {
        //         let builder = Arc::new(LeapfrogBuilder::new(PARALLELISM));
        //         input.iter().map(|batches| (Arc::clone(&builder), batches.clone())).collect()
        //     },
        //     |thread_index, (builder, batches)| async move { builder.run(thread_index, batches).await; },
        //     &all_keys,
        //     lookup_count,
        // );
        define_benchmark(
            &mut group,
            &rt,
            "Attempt3",
            || {
                let builder = Arc::new(ConcurrentBuilder::new(PARALLELISM));
                input.iter().map(|batches| (Arc::clone(&builder), batches.clone())).collect()
            },
            |thread_index, (builder, batches)| async move { builder.run(thread_index, batches).await },
            &all_keys,
            lookup_count,
        );
    }
}

criterion_main!(benches);
criterion_group! {
    name = benches;
    config = make_config();
    targets = criterion_benchmark
}
