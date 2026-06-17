#![feature(iter_array_chunks)]
#![feature(array_chunks)]
mod utils;

use crate::utils::input::make_vec;
use criterion::measurement::WallTime;
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkGroup, Criterion, Throughput};
use hashbrown::raw::raw_table_double_hash::RawTableDoubleHash;
use hashbrown::raw::raw_table_extra_bit::RawTableExtraBit;
use hashbrown::raw::RawTable;
use hashbrown_hashjoin::lookup::Lookup;
use hashbrown_hashjoin::new_map_3::fixed_table::ReadOnlyFixedTable;
use hashbrown_hashjoin::new_map_3::group::{BulkGroupStrategy, Group8, GroupStrategy};
use hashbrown_hashjoin::new_map_3::probe_sequence::ProbeSequenceBulk;
use rand::prelude::SliceRandom;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::future::Future;
use std::time::Duration;

fn make_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(5))
        .measurement_time(Duration::from_secs(10))
        .sample_size(50)
}


fn define_benchmark<L, T>(
    c: &mut BenchmarkGroup<WallTime>,
    rng: &mut impl Rng,
    name: &str,
    mut lookup: L,
    hash_count: usize)
where
    L: Fn(&[u64; 8]) -> T,
    T: Copy + Default
{
    assert_eq!(hash_count % 8, 0);

    c.throughput(Throughput::Elements(1000 * hash_count as u64));
    c.bench_function(name, |bencher| {
        bencher.iter_batched_ref(
            || {
                make_vec(hash_count, || rng.gen::<u64>()).array_chunks::<8>().copied().collect::<Vec<_>>()
            },
            |hash_chunks| {
                let mut output = vec![T::default(); hash_chunks.len()];
                for _ in 0..1000 {
                    for (chunk, output) in hash_chunks.iter().zip(output.iter_mut()) {
                        *output = lookup(chunk)
                    }
                }
                output
            },
            BatchSize::SmallInput,
        );
    });
}

fn criterion_benchmark(c: &mut Criterion) {
    let count = 65536;
    const CAPACITY: usize = 1usize << 8;
    const CAPACITY_MASK: usize = CAPACITY - 1;
    let tags_array = [0u8; CAPACITY + 8];
    let data_array = [(0u64, 0usize); CAPACITY];

    let mut group = c.benchmark_group("SimdOrLinear".to_string());

    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "Tags/Normal",
        |keys| {
            let mut output = [0u8; 8];
            for (key, output) in keys.iter().zip(output.iter_mut()) {
                *output = Group8::get_tag(*key);
            }
            output
        },
        count,
    );
    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "Tags/Simd",
        |keys| unsafe { Group8::get_tags(keys) },
        count,
    );

    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "ProbeStart/Normal",
        |keys| {
            let mut tags = [0u8; 8];
            let mut index = [0usize; 8];
            for ((key, tag_output), index_output) in keys.iter().zip(tags.iter_mut()).zip(index.iter_mut()) {
                *tag_output = Group8::get_tag(*key);
                *index_output = <Group8 as GroupStrategy>::ProbeSeq::start(*key, CAPACITY_MASK);
            }
            (tags, index)
        },
        count,
    );
    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "ProbeStart/Simd",
        |keys| unsafe {
            let capacity_mask = <Group8 as BulkGroupStrategy>::ProbeSeq::load_capacity_mask(CAPACITY_MASK);
            (Group8::get_tags(keys), <Group8 as BulkGroupStrategy>::ProbeSeq::start_indices(keys, capacity_mask))
        },
        count,
    );

    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "AllSetup/Normal",
        |keys| {
            let mut tags = [0u8; 8];
            let mut stored_hashes = [0u64; 8];
            let mut index = [0usize; 8];
            for (((key, tag_output), stored_hashes), index_output) in keys.iter().zip(tags.iter_mut()).zip(stored_hashes.iter_mut()).zip(index.iter_mut()) {
                *tag_output = Group8::get_tag(*key);
                *stored_hashes = *key | (1 << 63);
                *index_output = <Group8 as GroupStrategy>::ProbeSeq::start(*key, CAPACITY_MASK);
            }
            (tags, index)
        },
        count,
    );
    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "AllSetup/Simd",
        |keys| unsafe {
            let capacity_mask = <Group8 as BulkGroupStrategy>::ProbeSeq::load_capacity_mask(CAPACITY_MASK);
            let mut stored_hashes = [0u64; 8];
            for (index, key) in keys.iter().enumerate() {
                stored_hashes[index] = *key | (1 << 63);
            }
            (
                Group8::get_tags(keys),
                stored_hashes,
                <Group8 as BulkGroupStrategy>::ProbeSeq::start_indices(keys, capacity_mask),
            )
        },
        count,
    );

    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "Lookup/Normal",
        |keys| {
            let mut output = [None; 8];
            for (key, output) in keys.iter().zip(output.iter_mut()) {
                let search_tag = Group8::get_tag(*key);
                let stored_hash = *key | (1 << 63);
                let index = <Group8 as GroupStrategy>::ProbeSeq::start(*key, CAPACITY_MASK);
                unsafe {
                    let group = Group8::load_ptr(tags_array.as_ptr().add(index));
                    for position in Group8::match_tag(&group, search_tag) {
                        let item_index = (index + position) & CAPACITY_MASK;
                        let item = &*data_array.as_ptr().add(item_index);
                        if item.0 == stored_hash {
                            *output = Some(&item.1);
                        }
                    }
                }
            }
            output
        },
        count,
    );
    define_benchmark(
        &mut group,
        &mut make_seeded_random(),
        "Lookup/Simd",
        |keys| unsafe {
            let capacity_mask = <Group8 as BulkGroupStrategy>::ProbeSeq::load_capacity_mask(CAPACITY_MASK);
            let search_tags = Group8::get_tags(keys);
            let indexes = <Group8 as BulkGroupStrategy>::ProbeSeq::start_indices(keys, capacity_mask);
            let mut strides = [0usize; 8];

            let mut output = [None; 8];
            for ((((key, search_tag), index), mut stride), output) in keys.iter()
                .zip(search_tags.iter())
                .zip(indexes.iter())
                .zip(strides.iter_mut())
                .zip(output.iter_mut()) {
                unsafe {
                    let stored_hash = *key | (1 << 63);
                    let group = Group8::load_ptr(tags_array.as_ptr().add(*index as usize));
                    for position in Group8::match_tag(&group, *search_tag) {
                        let item_index = (*index as usize + position) & CAPACITY_MASK;
                        let item = &*data_array.as_ptr().add(item_index);
                        if item.0 == stored_hash {
                            *stride += 1;
                            *output = Some(&item.1);
                        }
                    }
                }
            }
            output
        },
        count,
    );
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

criterion_main!(benches);
criterion_group! {
    name = benches;
    config = make_config();
    targets = criterion_benchmark
}
