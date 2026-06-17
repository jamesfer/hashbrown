mod utils;
mod compare_ideal_probe_chains;

use crate::utils::input::make_input;
use hashbrown_hashjoin::new_map_2::new_map_2::WriteOnlyTable;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::future::Future;
use tokio::runtime::{Builder, Runtime};
use tokio::task::JoinSet;

// Number of power cores on an Apple M1 max
const PARALLELISM: usize = 8;


fn define_benchmark<P, I, R, F>(
    runtime: &Runtime,
    name: &str,
    mut prepare: P,
    mut run: R)
where
    P: FnMut() -> Vec<I>,
    R: FnMut(usize, I) -> F,
    F: Future<Output = ()> + Send + 'static,
{
    runtime.block_on(async {
        let inputs = prepare();

        let mut join_set = JoinSet::new();
        for (thread_index, input) in inputs.into_iter().enumerate() {
            join_set.spawn(run(thread_index, input));
        }

        join_set.join_all().await;
    });
}

fn main() {
    let rt = Builder::new_multi_thread()
        .worker_threads(PARALLELISM)
        .build()
        .unwrap();

    let mut rng = StdRng::seed_from_u64(44);
    let inputs = vec![
        // ("Random", make_input(PARALLELISM, 10, 1024, || rng.gen())),
        ("Random", make_input(PARALLELISM, 64, 8192, || rng.gen())),
        // ("Choice1024", make_input(PARALLELISM, 10, 256, limited_random(&mut rng, 1024))),
        // ("Choice1024", make_input(PARALLELISM, 64, 8192, limited_random(&mut rng, 1024))),
    ];
    for (name, input) in inputs {
        define_benchmark(
            &rt,
            "NewMap",
            || {
                let builder = WriteOnlyTable::new();
                input.iter().map(|batches| (builder.clone(), batches.clone())).collect()
            },
            |thread_index, (mut builder, batches)| async move {
                let mut offset = 0;
                for batch in batches {
                    for (index, hash) in batch.iter().enumerate() {
                        builder.insert(*hash, index + offset);
                    }
                    offset += batch.len();
                }

                builder.compact().await;
            },
        );
    }
}
