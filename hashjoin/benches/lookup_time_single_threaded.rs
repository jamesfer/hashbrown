#![feature(iter_array_chunks)]
#![feature(array_chunks)]
mod utils;

use std::collections::HashSet;
use crate::utils::input::make_vec;
use criterion::measurement::WallTime;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkGroup, BenchmarkId, Criterion, Throughput};
use hashbrown::raw::raw_table_double_hash::RawTableDoubleHash;
use hashbrown::raw::raw_table_extra_bit::RawTableExtraBit;
use hashbrown::raw::{Bucket, InsertSlot, RawTable};
use hashbrown_hashjoin::lookup::Lookup;
use hashbrown_hashjoin::new_map_3::fixed_table::{ReadOnlyFixedTable, WritableFixedTable};
use hashbrown_hashjoin::new_map_3::group::{BulkGroupStrategy, BulkGroupStrategy32, BulkGroupStrategyN, Group16, Group16Swiss, Group4, Group8, Group8SwissPlus, GroupStrategy, GroupType8SwissTable};
use rand::prelude::SliceRandom;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::future::Future;
use std::mem;
use std::time::Duration;

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
    mut prepare: P,
    mut lookup: L,
    hash_count: usize,
    lookup_count: usize)
where
    P: FnMut(&[u64]) -> T,
    L: Fn(&T, u64) -> Option<&usize>,
{
    // c.throughput(Throughput::Elements(lookup_count as u64));
    //
    // c.bench_function(name, |bencher| {
    //     bencher.iter_batched_ref(
    //         || {
    //             let hashes = make_vec(hash_count, || rng.gen::<u64>());
    //             let table = prepare(hashes.as_slice());
    //
    //             // Test that all values were written correctly
    //             for (index, hash) in hashes.iter().enumerate() {
    //                 let result = lookup(&table, *hash);
    //                 assert_eq!(result, Some(&index));
    //             }
    //
    //             let lookup_hashes = hashes.choose_multiple(rng, lookup_count)
    //                 .copied()
    //                 .collect::<Vec<_>>();
    //             (table, lookup_hashes)
    //         },
    //         |(table, keys_to_lookup)| {
    //             let mut output = Vec::with_capacity(keys_to_lookup.len());
    //             for key in keys_to_lookup.iter() {
    //                 output.push(lookup(&table, *key).copied());
    //             }
    //             output
    //         },
    //         BatchSize::SmallInput,
    //     );
    // });
    //
    //
    // c.bench_function(BenchmarkId::new(name, "Misses"), |bencher| {
    //     bencher.iter_batched_ref(
    //         || {
    //             let hashes = make_vec(hash_count, || rng.gen::<u64>());
    //             let table = prepare(hashes.as_slice());
    //
    //             let hash_set = hashes.iter().cloned().collect::<HashSet<_>>();
    //             let miss_hashes: Vec<_> = std::iter::repeat_with(|| rng.gen::<u64>())
    //                 .filter(|hash| !hash_set.contains(hash))
    //                 .take(lookup_count)
    //                 .collect();
    //
    //             // Test that all values were written correctly
    //             for hash in miss_hashes.iter() {
    //                 let result = lookup(&table, *hash);
    //                 assert_eq!(result, None);
    //             }
    //
    //             (table, miss_hashes)
    //         },
    //         |(table, keys_to_lookup)| {
    //             let mut output = Vec::with_capacity(keys_to_lookup.len());
    //             for key in keys_to_lookup.iter() {
    //                 output.push(lookup(&table, *key).copied());
    //             }
    //             output
    //         },
    //         BatchSize::SmallInput,
    //     );
    // });

    c.throughput(Throughput::Elements(2 * lookup_count as u64));
    c.bench_function(BenchmarkId::new(name, "50%Misses"), |bencher| {
        bencher.iter_batched_ref(
            || {
                let hashes = make_vec(hash_count, || rng.gen::<u64>());
                let table = prepare(hashes.as_slice());

                // Test that all values were written correctly
                for (index, hash) in hashes.iter().enumerate() {
                    let result = lookup(&table, *hash);
                    assert_eq!(result, Some(&index));
                }

                let hash_set = hashes.iter().cloned().collect::<HashSet<_>>();
                let miss_hashes: Vec<_> = std::iter::repeat_with(|| rng.gen::<u64>())
                    .filter(|hash| !hash_set.contains(hash))
                    .take(lookup_count)
                    .collect();
                for hash in miss_hashes.iter() {
                    let result = lookup(&table, *hash);
                    assert_eq!(result, None);
                }

                let lookup_hashes = hashes.choose_multiple(rng, lookup_count)
                    .copied()
                    .collect::<Vec<_>>();

                let mut all_hashes = Vec::with_capacity(lookup_hashes.len() + miss_hashes.len());
                all_hashes.extend(lookup_hashes.iter().copied());
                all_hashes.extend(miss_hashes.iter().copied());
                all_hashes.shuffle(rng);

                (table, all_hashes)
            },
            |(table, keys_to_lookup)| {
                let mut output = Vec::with_capacity(keys_to_lookup.len());
                for key in keys_to_lookup.iter() {
                    output.push(lookup(&table, *key).copied());
                }
                output
            },
            BatchSize::SmallInput,
        );
    });
}

// fn define_bulk_benchmark<P, T, L>(
//     c: &mut BenchmarkGroup<WallTime>,
//     rng: &mut impl Rng,
//     name: &str,
//     mut prepare: P,
//     mut lookup: L,
//     hash_count: usize,
//     lookup_count: usize)
// where
//     P: FnMut(&[u64]) -> T,
//     L: for<'a> Fn(&'a T, &[u64]) -> Vec<Option<&'a usize>>,
// {
//     c.throughput(Throughput::Elements(lookup_count as u64));
//
//     c.bench_function(name, |bencher| {
//         bencher.iter_batched_ref(
//             || {
//                 let hashes = make_vec(hash_count, || rng.gen::<u64>());
//                 let table = prepare(hashes.as_slice());
//
//                 // Test that all values were written correctly
//                 let results = lookup(&table, hashes.as_slice());
//                 assert_eq!(results.len(), hashes.len());
//                 for (index, result) in results.into_iter().enumerate() {
//                     assert_eq!(result, Some(&index));
//                 }
//
//                 let lookup_hashes = hashes.choose_multiple(rng, lookup_count)
//                     .copied()
//                     .collect::<Vec<_>>();
//
//                 (table, lookup_hashes)
//             },
//             |(table, keys_to_lookup)| {
//                 let mut output = Vec::with_capacity(keys_to_lookup.len());
//                 for chunk in keys_to_lookup.chunks(32) {
//                     output.extend(lookup(&table, chunk).iter().map(|x| x.copied()));
//                 }
//                 output
//             },
//             BatchSize::SmallInput,
//         );
//     });
//
//
//     c.bench_function(BenchmarkId::new(name, "Misses"), |bencher| {
//         bencher.iter_batched_ref(
//             || {
//                 let hashes = make_vec(hash_count, || rng.gen::<u64>());
//                 let table = prepare(hashes.as_slice());
//
//                 // Test that all values were written correctly
//                 let results = lookup(&table, hashes.as_slice());
//                 assert_eq!(results.len(), hashes.len());
//                 for (index, result) in results.into_iter().enumerate() {
//                     assert_eq!(result, Some(&index));
//                 }
//
//                 let hash_set = hashes.iter().cloned().collect::<HashSet<_>>();
//                 let miss_hashes: Vec<_> = std::iter::repeat_with(|| rng.gen::<u64>())
//                     .filter(|hash| !hash_set.contains(hash))
//                     .take(lookup_count)
//                     .collect();
//
//                 // Test that all other hashes are missed
//                 let results = lookup(&table, miss_hashes.as_slice());
//                 assert_eq!(results.len(), miss_hashes.len());
//                 for result in results.into_iter() {
//                     assert_eq!(result, None);
//                 }
//
//                 (table, miss_hashes)
//             },
//             |(table, keys_to_lookup)| {
//                 let mut output = Vec::with_capacity(keys_to_lookup.len());
//                 for chunk in keys_to_lookup.chunks(32) {
//                     output.extend(lookup(&table, chunk).iter().map(|x| x.copied()));
//                 }
//                 output
//             },
//             BatchSize::SmallInput,
//         );
//     });
// }

fn define_fixed_bulk_benchmark<const N: usize, P, T, L>(
    c: &mut BenchmarkGroup<WallTime>,
    rng: &mut impl Rng,
    name: &str,
    mut prepare: P,
    mut lookup: L,
    hash_count: usize,
    lookup_count: usize)
where
    P: FnMut(&[u64]) -> T,
    L: for<'a> Fn(&'a T, &[u64; N], &mut [Option<usize>; N]),
{
    c.throughput(Throughput::Elements(2 * lookup_count as u64));
    c.bench_function(BenchmarkId::new(name, "50%Misses"), |bencher| {
        bencher.iter_batched_ref(
            || {
                let hashes = make_vec(hash_count, || rng.gen::<u64>());
                let table = prepare(hashes.as_slice());

                // Test that all values were written correctly
                for (chunk_index, chunk) in hashes.array_chunks::<N>().enumerate() {
                    let mut results = [None; N];
                    lookup(&table, chunk, &mut results);
                    for (index, result) in results.into_iter().enumerate() {
                        assert_eq!(result, Some(chunk_index * N + index));
                    }
                }

                let hash_set = hashes.iter().cloned().collect::<HashSet<_>>();
                let miss_hashes: Vec<_> = std::iter::repeat_with(|| rng.gen::<u64>())
                    .filter(|hash| !hash_set.contains(hash))
                    .take(lookup_count)
                    .collect();
                // Test that all other hashes are missed
                for chunk in miss_hashes.array_chunks::<N>() {
                    let mut results = [None; N];
                    lookup(&table, chunk, &mut results);
                    for result in results.into_iter() {
                        assert_eq!(result, None);
                    }
                }

                let lookup_hashes = hashes.choose_multiple(rng, lookup_count)
                    .copied()
                    .collect::<Vec<_>>();

                let mut all_hashes = Vec::with_capacity(lookup_hashes.len() + miss_hashes.len());
                all_hashes.extend(lookup_hashes.iter().copied());
                all_hashes.extend(miss_hashes.iter().copied());
                all_hashes.shuffle(rng);

                (table, all_hashes)
            },
            |(table, keys_to_lookup)| {
                let mut output = vec![None; keys_to_lookup.len()];
                for (chunk, output) in keys_to_lookup.array_chunks::<N>().zip(output.array_chunks_mut::<N>()) {
                    lookup(&table, chunk, output);
                }
                output
            },
            BatchSize::SmallInput,
        );
    });
}

fn criterion_benchmark(criterion: &mut Criterion) {
    // Size must be 16 * 2^n (16 * 2 ^ 12)
    let table_capacity = 2usize.pow(16);
    let lookup_count = 8192;

    for load_ratio in [
        // 0.5,
        0.75,
        // 0.875,
    ] {
        define_all_benchmarks(
            criterion,
            table_capacity,
            load_ratio,
            lookup_count,
        );
    }
}

fn define_all_benchmarks(
    criterion: &mut Criterion,
    table_capacity: usize,
    load_ratio: f64,
    lookup_count: usize,
) {
    let value_count = (table_capacity as f64 * load_ratio) as usize;

    let effective_load_ratio = value_count as f64 / table_capacity as f64;
    assert!(effective_load_ratio <= load_ratio, "Effective load ratio {effective_load_ratio} must be lower than testing load ratio {load_ratio}");
    assert!(effective_load_ratio > load_ratio - 0.01);

    // Rust's tables always have a capacity of a power of 2, but they have a maximum load factor of
    // 87.5%, so their usable capacity will be less.
    // NewMap's tables have a capacity of 16*2^n, and they can use a load factor of 100%.
    let hash_map_requested_capacity = (table_capacity as f64 * 0.875) as usize;

    let group_name = format!("SingleThreadedLookupThroughput/size:{table_capacity}/load:{load_ratio}");
    let mut group = criterion.benchmark_group(group_name);
    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "HashMap",
        |hashes| {
            // Rust's hashmap has an internal load factor of 87.5%
            let mut table = RawTable::with_capacity(hash_map_requested_capacity);
            assert_eq!(table.buckets(), table_capacity);

            for (index, hash) in hashes.iter().enumerate() {
                table.insert(*hash, (*hash, index), |(hash, _)| *hash);
            }

            // Confirm that the size of the table is still correct
            assert_eq!(table.buckets(), table_capacity);

            table
        },
        lookup_raw_table,
        value_count,
        lookup_count,
    );

    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "Dashmap",
        |hashes| {
            // Rust's hashmap has an internal load factor of 87.5%
            const SHARDS: usize = 16;
            let capacity_per_shard = hash_map_requested_capacity / SHARDS;
            let mut tables: [RawTable<(u64, usize)>; SHARDS] = std::array::from_fn(|_| RawTable::with_capacity(capacity_per_shard));

            for (index, hash) in hashes.iter().enumerate() {
                write_dash_map(&mut tables, *hash, index);
            }

            tables
        },
        lookup_dash_map,
        value_count,
        lookup_count,
    );

    // define_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "HashMap (double hash)",
    //     |hashes| {
    //         // Rust's hashmap has an internal load factor of 87.5%
    //         let mut table = RawTableDoubleHash::with_capacity(hash_map_requested_capacity);
    //         assert_eq!(table.buckets(), table_capacity);
    //
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, (*hash, index), |(hash, _)| *hash);
    //         }
    //
    //         // Confirm that the size of the table is still correct
    //         assert_eq!(table.buckets(), table_capacity);
    //
    //         vec![0; table.buckets()];
    //
    //         table
    //     },
    //     lookup_raw_table_new_seq,
    //     value_count,
    //     lookup_count,
    // );

    // // define_benchmark(
    // //     &mut group,
    // //     &mut StdRng::seed_from_u64(44),
    // //     "HashMap (extra bit)",
    // //     |hashes| {
    // //         // Rust's hashmap has an internal load factor of 87.5%
    // //         let mut table = RawTableExtraBit::with_capacity(hash_map_requested_capacity);
    // //         assert_eq!(table.buckets(), table_capacity);
    // //
    // //         for (index, hash) in hashes.iter().enumerate() {
    // //             table.insert(*hash, (*hash, index), |(hash, _)| *hash);
    // //         }
    // //
    // //         // Confirm that the size of the table is still correct
    // //         assert_eq!(table.buckets(), table_capacity);
    // //
    // //         table
    // //     },
    // //     lookup_raw_table_extra,
    // //     value_count,
    // //     lookup_count,
    // // );
    //
    // // define_benchmark(
    // //     &mut group,
    // //     &mut StdRng::seed_from_u64(44),
    // //     "FixedMap2",
    // //     |hashes| {
    // //         let mut table = FixedTable::new_with_capacity(table_capacity);
    // //         for (index, hash) in hashes.iter().enumerate() {
    // //             table.write(*hash, index).unwrap();
    // //         }
    // //
    // //         let read_only_table = table.to_read_only();
    // //
    // //         read_only_table
    // //     },
    // //     |table, key| { table.get(key) },
    // //     value_count,
    // //     lookup_count,
    // // );

    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "FixedMap3 (group size 8)",
        |hashes| {
            let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(table_capacity);
            for (index, hash) in hashes.iter().enumerate() {
                table.insert(*hash, index).unwrap();
            }

            let read_only_table = table.to_read_only();

            read_only_table
        },
        lookup_fixed_table,
        value_count,
        lookup_count,
    );

    // define_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "FixedMap3 (group size 16)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group16> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table,
    //     value_count,
    //     lookup_count,
    // );

    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "FixedMap3 (group size 8 swiss table)",
        |hashes| {
            let mut table: WritableFixedTable<_, GroupType8SwissTable> = WritableFixedTable::with_capacity(table_capacity);
            for (index, hash) in hashes.iter().enumerate() {
                table.insert(*hash, index).unwrap();
            }

            let read_only_table = table.to_read_only();

            read_only_table
        },
        lookup_fixed_table,
        value_count,
        lookup_count,
    );

    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "FixedMap3 (group size 8 swiss table plus)",
        |hashes| {
            let mut table: WritableFixedTable<_, Group8SwissPlus> = WritableFixedTable::with_capacity(table_capacity);
            for (index, hash) in hashes.iter().enumerate() {
                table.insert(*hash, index).unwrap();
            }

            let read_only_table = table.to_read_only();

            read_only_table
        },
        lookup_fixed_table,
        value_count,
        lookup_count,
    );

    // // define_benchmark(
    // //     &mut group,
    // //     &mut make_seeded_random(),
    // //     "FixedMap3 (group size 8 swiss table plus const lookup)",
    // //     |hashes| {
    // //         let mut table: WritableFixedTable<_, Group8SwissPlus> = WritableFixedTable::with_capacity(table_capacity);
    // //         for (index, hash) in hashes.iter().enumerate() {
    // //             table.insert(*hash, index).unwrap();
    // //         }
    // //
    // //         let read_only_table = table.to_read_only();
    // //
    // //         read_only_table
    // //     },
    // //     const_lookup_fixed_table,
    // //     value_count,
    // //     lookup_count,
    // // );
    //
    // define_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "FixedMap3 (group size 16 swiss table)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group16Swiss> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table,
    //     value_count,
    //     lookup_count,
    // );

    // // define_benchmark(
    // //     &mut group,
    // //     &mut StdRng::seed_from_u64(44),
    // //     "FixedMap8",
    // //     |hashes| {
    // //         let mut table = FixedTable8::new_with_capacity(table_capacity);
    // //         for (index, hash) in hashes.iter().enumerate() {
    // //             table.write(*hash, index).unwrap();
    // //         }
    // //
    // //         let read_only_table = table.to_read_only();
    // //
    // //         // Test
    // //         for (index, hash) in hashes.iter().enumerate() {
    // //             let result = read_only_table.get(*hash);
    // //             assert_eq!(result.copied(), Some(index));
    // //         }
    // //
    // //         read_only_table
    // //     },
    // //     |table, key| { table.get(key) },
    // //     value_count,
    // //     lookup_count,
    // // );


    /////////////////////////////////
    // Bulk

    // define_fixed_bulk_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 8, using fixed group)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk_with_fixed_group,
    //     value_count,
    //     lookup_count,
    // );

    // define_fixed_bulk_benchmark::<8, _, _, _>(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 8, using 8 fixed group)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk_with_fixed_group_n,
    //     value_count,
    //     lookup_count,
    // );

    // define_fixed_bulk_benchmark::<32, _, _, _>(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 8, using 32 fixed group)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk_with_fixed_group_n,
    //     value_count,
    //     lookup_count,
    // );

    // define_fixed_bulk_benchmark::<256, _, _, _>(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 8, using 256 fixed group)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk_with_fixed_group_n,
    //     value_count,
    //     lookup_count,
    // );
    //
    // define_fixed_bulk_benchmark::<1024, _, _, _>(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 8, using 1024 fixed group)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk_with_fixed_group_n,
    //     value_count,
    //     lookup_count,
    // );


    // define_fixed_bulk_benchmark::<32, _, _, _>(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 8, using 32b fixed group)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk_with_fixed_group_n_b,
    //     value_count,
    //     lookup_count,
    // );

    // define_fixed_bulk_benchmark::<32, _, _, _>(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 4, using 32b fixed group)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group4> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk_with_fixed_group_n_b,
    //     value_count,
    //     lookup_count,
    // );


    // define_bulk_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 8)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk,
    //     value_count,
    //     lookup_count,
    // );
    //
    // define_bulk_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulkStatic/FixedMap3 (group size 8)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk_static,
    //     value_count,
    //     lookup_count,
    // );

    // define_bulk_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 16)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group16> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk,
    //     value_count,
    //     lookup_count,
    // );
    //
    // define_bulk_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 8 swiss table)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, GroupType8SwissTable> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk,
    //     value_count,
    //     lookup_count,
    // );
    //
    // define_bulk_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 8 swiss table plus)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group8SwissPlus> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk,
    //     value_count,
    //     lookup_count,
    // );
    //
    // define_bulk_benchmark(
    //     &mut group,
    //     &mut make_seeded_random(),
    //     "InBulk/FixedMap3 (group size 16 swiss table)",
    //     |hashes| {
    //         let mut table: WritableFixedTable<_, Group16Swiss> = WritableFixedTable::with_capacity(table_capacity);
    //         for (index, hash) in hashes.iter().enumerate() {
    //             table.insert(*hash, index).unwrap();
    //         }
    //
    //         let read_only_table = table.to_read_only();
    //
    //         read_only_table
    //     },
    //     lookup_fixed_table_in_bulk,
    //     value_count,
    //     lookup_count,
    // );
}

fn make_seeded_random() -> StdRng {
    StdRng::seed_from_u64(44)
}

#[inline(never)]
fn lookup_fixed_table<T: GroupStrategy>(table: &ReadOnlyFixedTable<usize, T>, key: u64) -> Option<&usize> {
    table.get(key)
}

#[inline(never)]
fn lookup_fixed_table_in_bulk<'a, T: GroupStrategy>(table: &'a ReadOnlyFixedTable<usize, T>, keys: &[u64]) -> Vec<Option<&'a usize>> {
    table.get_in_bulk(keys)
}

#[inline(never)]
fn lookup_fixed_table_in_bulk_with_fixed_group<'a, T: BulkGroupStrategy>(
    table: &'a ReadOnlyFixedTable<usize, T>,
    keys: &[u64; 8],
) -> [Option<&'a usize>; 8] {
    table.get_in_bulk_static_8(keys)
}

#[inline(never)]
fn lookup_fixed_table_32_in_bulk_with_fixed_group<'a, T: BulkGroupStrategy32>(
    table: &'a ReadOnlyFixedTable<usize, T>,
    keys: &[u64; 32],
) -> [Option<&'a usize>; 32] {
    table.get_in_bulk_static_32(keys)
}

#[inline(never)]
fn lookup_fixed_table_in_bulk_with_fixed_group_n<'a, const N: usize, T: BulkGroupStrategyN>(
    table: &'a ReadOnlyFixedTable<usize, T>,
    keys: &[u64; N],
    output: &mut [Option<usize>; N],
) {
    table.get_in_bulk_static_n(keys, output)
}

#[inline(never)]
fn lookup_fixed_table_in_bulk_with_fixed_group_n_b<'a, const N: usize, T: BulkGroupStrategyN>(
    table: &'a ReadOnlyFixedTable<usize, T>,
    keys: &[u64; N],
    output: &mut [Option<usize>; N],
) {
    table.get_in_bulk_static_n_b(keys, output)
}


#[inline(never)]
fn lookup_fixed_table_in_bulk_static<'a, T: GroupStrategy>(table: &'a ReadOnlyFixedTable<usize, T>, keys: &[u64]) -> Vec<Option<&'a usize>> {
    let mut output = Vec::with_capacity(keys.len());
    let mut iter = keys.iter().array_chunks::<256>();
    while let Some(chunk) = iter.next() {
        output.extend(table.get_in_bulk_static(chunk))
    }

    if let Some(remainder) = iter.into_remainder() {
        for key in remainder {
            output.push(table.get(*key));
        }
    }

    output
}

#[inline(never)]
fn const_lookup_fixed_table<T: GroupStrategy>(table: &ReadOnlyFixedTable<usize, T>, key: u64) -> Option<&usize> {
    table.get_const_lookup(key)
}

#[no_mangle]
#[inline(never)]
fn lookup_raw_table(table: &RawTable<(u64, usize)>, key: u64) -> Option<&usize> {
    match table.get(key, |(hash, _)| *hash == key) {
        None => None,
        Some((_, value)) => Some(value)
    }
}

#[no_mangle]
#[inline(never)]
fn lookup_raw_table_new_seq(table: &RawTableDoubleHash<(u64, usize)>, key: u64) -> Option<&usize> {
    match table.get(key, |(hash, _)| *hash == key) {
        None => None,
        Some((_, value)) => Some(value)
    }
}

#[no_mangle]
#[inline(never)]
fn lookup_raw_table_extra(table: &RawTableExtraBit<(u64, usize)>, key: u64) -> Option<&usize> {
    match table.get(key, |(hash, _)| *hash == key) {
        None => None,
        Some((_, value)) => Some(value)
    }
}

#[inline(never)]
fn lookup_dash_map<const SHARDS: usize>(tables: &[RawTable<(u64, usize)>; SHARDS], key: u64) -> Option<&usize> {
    let idx = (key >> (64 - 7)) as usize & (SHARDS - 1);
    match tables[idx].get(key, |(hash, _)| *hash == key) {
        None => None,
        Some((_, value)) => Some(value)
    }
}

fn write_dash_map<const SHARDS: usize>(tables: &mut [RawTable<(u64, usize)>; SHARDS], key: u64, value: usize) -> Option<usize> {
    let idx = (key >> (64 - 7)) as usize & (SHARDS - 1);
    let table = &mut tables[idx];
    match table.find_or_find_insert_slot(key, |(hash, _)| *hash == key, |(hash, _)| *hash) {
        Ok(bucket) => {
            Some(mem::replace(unsafe { &mut bucket.as_mut().1 }, value))
        }
        Err(slot) => {
            unsafe { table.insert_in_slot(key, slot, (key, value)) };
            None
        }
    }
}

criterion_main!(benches);
criterion_group! {
    name = benches;
    config = make_config();
    targets = criterion_benchmark
}
