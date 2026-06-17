use rand::prelude::SliceRandom;
use rand::Rng;

pub fn limited_random<R: Rng>(rng: &mut R, num: usize) -> impl FnMut() -> u64 + '_ {
    let choices = (0..num).into_iter().map(|x| rng.gen()).collect::<Vec<u64>>();
    move || *choices.choose(rng).unwrap()
}

pub fn make_input(parallelism: usize, batches: usize, batch_size: usize, mut generator: impl FnMut() -> u64) -> Vec<Vec<Vec<u64>>> {
    (0..parallelism).into_iter()
        .map(|thread_index| {
            (0..batches).into_iter()
                .map(|batch_index| {
                    // let min = thread_index * batch_size + batch_index * parallelism * batch_size;
                    (0..batch_size).into_iter().map(|_| generator()).collect::<Vec<u64>>()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
}
