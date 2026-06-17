mod utils;
mod compare_ideal_probe_chains;

use hashbrown::raw::RawTable;
use hashbrown_hashjoin::new_map_2::fixed_table::{FixedTable, ReadOnlyTable};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::future::Future;

fn define_benchmark<P, T, R>(
    rng: &mut impl Rng,
    hash_count: usize,
    mut prepare: P,
    mut lookup: R)
where
    P: FnMut(&[u64]) -> T,
    R: FnMut(u64, &T) -> Option<&usize>,
{
    let hashes = (0..hash_count).map(|_| rng.gen::<u64>()).collect::<Vec<_>>();
    let table = prepare(&hashes);
    let mut sum = 0;

    for _ in 0..100_000 {
        for key in hashes.iter().copied() {
            if let Some(x) = lookup(key, &table) {
                sum += x;
            };
        }
    }

    println!("Sum: {}", sum)
}

fn main() {
    let load_ratio = 0.5;
    let table_capacity = 16384 * 2;

    let value_count = (table_capacity as f64 * load_ratio) as usize;
    let hash_map_requested_capacity = (table_capacity as f64 * load_ratio) as usize;
    let effective_load_ratio = value_count as f64 / table_capacity as f64;
    assert!(effective_load_ratio <= load_ratio, "Effective load ratio {effective_load_ratio} must be lower than testing load ratio {load_ratio}");
    assert!(effective_load_ratio > load_ratio - 0.01);

    define_benchmark(
        &mut StdRng::seed_from_u64(44),
        value_count,
        |hashes| {
            let mut builder = RawTable::with_capacity(hash_map_requested_capacity);
            for (index, hash) in hashes.iter().enumerate() {
                builder.insert(*hash, (*hash, index), |(hash, _)| *hash);
            }
            builder
        },
        lookup_raw_table,
    );

    define_benchmark(
        &mut StdRng::seed_from_u64(44),
        value_count,
        |hashes| {
            let builder = FixedTable::new(table_capacity);
            for (index, hash) in hashes.iter().enumerate() {
                builder.write(*hash, index).unwrap();
            }
            builder.to_read_only()
        },
        lookup_fixed_table,
    );
}

#[inline(never)]
fn lookup_fixed_table(hash: u64, table: &ReadOnlyTable<usize>) -> Option<&usize> {
    table.get(hash)
}

#[inline(never)]
fn lookup_raw_table(hash: u64, table: &RawTable<(u64, usize)>) -> Option<&usize> {
    match table.get(hash, |(stored_hash, _)| hash == *stored_hash) {
        None => None,
        Some((_, v)) => Some(v),
    }
}
