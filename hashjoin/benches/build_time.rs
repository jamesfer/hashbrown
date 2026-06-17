mod utils;

use std::future::Future;
use std::iter;
use std::sync::Arc;
use std::time::Duration;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkGroup, Criterion};
use criterion::measurement::WallTime;
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinSet;
use hashbrown_hashjoin::builder::attempt3::ConcurrentBuilder;
use hashbrown_hashjoin::dash_map_builder::DashMapBuilder;
use hashbrown_hashjoin::leapfrog_builder::LeapfrogBuilder;
use hashbrown_hashjoin::mutex_map_builder::MutexMapBuilder;
use hashbrown_hashjoin::new_map_3::new_map_3::WriteOnlyTable;
use crate::utils::input::{limited_random, make_input};

// Number of power cores on an Apple M1 max
const PARALLELISM: usize = 8;

fn make_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(10))
        .measurement_time(Duration::from_secs(30))
        .sample_size(10)
}

fn define_benchmark<P, I, R, F>(
    c: &mut BenchmarkGroup<WallTime>,
    runtime: &Runtime,
    name: &str,
    mut prepare: P,
    mut run: R)
where
    P: FnMut() -> Vec<I>,
    R: FnMut(usize, I) -> F,
    F: Future<Output = ()> + Send + 'static,
{
    c.bench_function(name, |bencher| {
        bencher.to_async(runtime).iter_batched(
            || {
                let inputs = prepare();
                inputs
            },
            |input| {
                let mut join_set = JoinSet::new();
                for (thread_index, input) in input.into_iter().enumerate() {
                    join_set.spawn(run(thread_index, input));
                }
                join_set.join_all()
            },
            BatchSize::SmallInput,
        )
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    let rt = Builder::new_multi_thread()
        .worker_threads(PARALLELISM)
        .build()
        .unwrap();

    let inputs = vec![
        // ("Random", make_input(PARALLELISM, 10, 1024, rand::random)),
        // ("Random", make_input(PARALLELISM, 64, 8192, rand::random)),
        ("Choice1024", make_input(PARALLELISM, 10, 256, limited_random(1024))),
        // ("Choice1024", make_input(PARALLELISM, 64, 8192, limited_random(1024))),
    ];
    for (name, input) in inputs {
        let total_size = input.iter().flatten().flatten().count();
        let mut group = c.benchmark_group(format!("BuildTime/{name}/Size{total_size}"));
        // define_benchmark(
        //     &mut group,
        //     &rt,
        //     "MutexMap",
        //     || {
        //         let builder = Arc::new(MutexMapBuilder::new(PARALLELISM));
        //         input.iter().map(|batches| (Arc::clone(&builder), batches.clone())).collect()
        //     },
        //     |thread_index, (builder, batches)| async move { builder.run(thread_index, batches).await; },
        // );
        define_benchmark(
            &mut group,
            &rt,
            "DashMap",
            || {
                let builder = Arc::new(DashMapBuilder::new(PARALLELISM));
                input.iter().map(|batches| (Arc::clone(&builder), batches.clone())).collect()
            },
            |thread_index, (builder, batches)| async move { builder.run(thread_index, batches).await; },
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
        // );
        // define_benchmark(
        //     &mut group,
        //     &rt,
        //     "Attempt3",
        //     || {
        //         let builder = Arc::new(ConcurrentBuilder::new(PARALLELISM));
        //         input.iter().map(|batches| (Arc::clone(&builder), batches.clone())).collect()
        //     },
        //     |thread_index, (builder, batches)| async move { builder.run(thread_index, batches).await; },
        // );
        define_benchmark(
            &mut group,
            &rt,
            "NewMap3",
            || {
                let builder: WriteOnlyTable<usize> = WriteOnlyTable::new();
                input.iter()
                    .zip(iter::repeat(builder))
                    .map(|(batches, builder)| (batches.clone(), builder))
                    .collect()
            },
            |_thread_index, (batches, mut builder)| async move {
                for batch in batches {
                    for (index, hash) in batch.iter().enumerate() {
                        builder.insert(*hash, index);
                    }
                }

                builder.compact().await;
            }
        )
    }
}

criterion_main!(benches);
criterion_group! {
    name = benches;
    config = make_config();
    targets = criterion_benchmark
}
