use std::iter;
use tokio::task::JoinSet;
use hashbrown_hashjoin::new_map_3::new_map_3::WriteOnlyTable;

const PARALLELISM: usize = 8;

pub fn make_vec(batch_size: usize, mut generator: impl FnMut() -> u64 + Sized) -> Vec<u64> {
    (0..batch_size).into_iter().map(|_| generator()).collect::<Vec<u64>>()
}

pub fn make_input(parallelism: usize, batches: usize, batch_size: usize, mut generator: impl FnMut() -> u64) -> Vec<Vec<Vec<u64>>> {
    (0..parallelism).into_iter()
        .map(|_thread_index| {
            (0..batches).into_iter()
                .map(|_batch_index| {
                    // let min = thread_index * batch_size + batch_index * parallelism * batch_size;
                    make_vec(batch_size, || generator())
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    for i in 0..10 {
        println!("Iteration {}", i);

        let input = make_input(PARALLELISM, 128, 8192, rand::random);
        let builder: WriteOnlyTable<usize> = WriteOnlyTable::new();
        let contexts: Vec<_> = input.iter()
            .zip(iter::repeat(builder))
            .map(|(batches, builder)| (batches.clone(), builder))
            .collect();

        let mut join_set = JoinSet::new();
        for (_thread_index, (batches, mut builder)) in contexts.into_iter().enumerate() {
            join_set.spawn(async move {
                for batch in batches {
                    for (index, hash) in batch.iter().enumerate() {
                        builder.insert(*hash, index + 1);
                    }
                }

                println!("Finished building, {} failed writes from thread {}", builder.failed_writes(), _thread_index);
                builder.compact().await;
            });
        }
        join_set.join_all().await;
    }
}
