use hashbrown::raw::raw_table_double_hash::RawTableDoubleHash;
use hashbrown::raw::raw_table_extra_bit::RawTableExtraBit;
use hashbrown::raw::RawTable;
use hashbrown_hashjoin::new_map_2::fixed_table::FixedTable;
use hashbrown_hashjoin::new_map_3::fixed_table::WritableFixedTable;
use hashbrown_hashjoin::new_map_3::group::{Group16, Group16Swiss, Group8, GroupType8SwissTable};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashSet;

const ITERATIONS: usize = 400;
// For the original fixed table, the capacity needs to be 16 * 2^n
const CAPACITY: usize = 16 * 2usize.pow(10);
const LOAD_RATIOS: [f64; 3] = [
    0.5,
    0.75,
    0.875,
];

pub struct Stats {
    pub probe_length: usize,
    pub false_positives: usize,
}

struct Result {
    avg_probe_length: f64,
    avg_false_positives: f64,
    avg_miss_probe_length: f64,
    avg_miss_false_positives: f64,
}

type Results<'a> = (&'a str, Vec<Result>);

fn test<M, L, T>(
    name: &str,
    mut make: M,
    mut lookup: L,
) -> Results
where
    M: FnMut(usize, &[(u64, usize)]) -> T,
    L: FnMut(u64, &T) -> (Option<&usize>, Stats),
{
    let mut rng = StdRng::seed_from_u64(0);

    let results: Vec<_> = LOAD_RATIOS.iter()
        .map(|load_ratio| {
            let mut total_probe_length = 0usize;
            let mut total_false_positives = 0usize;
            let mut total_miss_probe_length = 0usize;
            let mut total_miss_false_positives = 0usize;

            let value_count = (CAPACITY as f64 * load_ratio) as usize;
            for _ in 0..ITERATIONS {
                // Create some hashes
                let mut hashes = vec![0u64; value_count];
                rng.fill(&mut hashes[..]);
                let pairs: Vec<_> = hashes.iter().cloned().enumerate().map(|(index, hash)| (hash, index)).collect();

                // Build the table
                let table = make(CAPACITY, &pairs);

                // Check each of the hashes in the table
                for (hash, value) in &pairs {
                    let (result, stats) = lookup(*hash, &table);
                    assert_eq!(result, Some(value));

                    total_probe_length += stats.probe_length;
                    total_false_positives += stats.false_positives;
                }

                // Check hashes that don't exist
                let hash_set = hashes.iter().cloned().collect::<HashSet<_>>();
                let miss_hashes: Vec<_> = std::iter::repeat_with(|| rng.gen::<u64>())
                    .filter(|hash| !hash_set.contains(hash))
                    .take(value_count)
                    .collect();
                for hash in &miss_hashes {
                    let (result, stats) = lookup(*hash, &table);
                    assert_eq!(result, None);

                    total_miss_probe_length += stats.probe_length;
                    total_miss_false_positives += stats.false_positives;
                }
            }


            let total_value_count = value_count * ITERATIONS;

            Result {
                avg_probe_length: total_probe_length as f64 / total_value_count as f64,
                avg_false_positives: total_false_positives as f64 / total_value_count as f64,
                avg_miss_probe_length: total_miss_probe_length as f64 / total_value_count as f64,
                avg_miss_false_positives: total_miss_false_positives as f64 / total_value_count as f64,
            }


            // println!("{} (capacity {}, load ratio {}): ", name, capacity, load_ratio);
            // println!("                         Hits        Misses");
            // println!("  Avg probe length:      {:.5}      {:.5}", total_probe_length as f64 / total_value_count as f64, total_miss_probe_length as f64 / total_value_count as f64);
            // println!("  Avg false positives:   {:.5}      {:.5}", total_false_positives as f64 / total_value_count as f64, total_miss_false_positives as f64 / total_value_count as f64);
        })
        .collect();


    (name, results)
}

fn display(results: Vec<Results>) {
    println!("                     Probe length                    False positives");
    println!("                     Hits          Misses            Hits          Misses");

    for (index, load_ratio) in LOAD_RATIOS.iter().enumerate() {
        println!("{load_ratio}");
        for (name, stats) in &results {
            let stats = &stats[index];
            println!("  {: <18} {: <13.6} {: <13.6}     {: <13.6} {: <13.6}", name, stats.avg_probe_length, stats.avg_miss_probe_length, stats.avg_false_positives, stats.avg_miss_false_positives);
        }
    }
}

pub fn main() {
    let mut results = Vec::new();
    results.push(test(
        "RawTable",
        |capacity, pairs| {
            // Need to adjust the requested capacity of Rust's table
            let mut table = RawTable::with_capacity((capacity as f64 * 0.875) as usize);
            for (hash, index) in pairs {
                table.insert(*hash, (*hash, *index), |(hash, _)| *hash);
            }
            table
        },
        |hash, table| {
            let (value, probe_length, false_positives) = table.get_with_stats(hash, |(stored_hash, _)| *stored_hash == hash);
            let output = value.map(|(_, index)| index);
            (output, Stats { probe_length, false_positives })
        },
    ));
    results.push(test(
        "RawTable (dblHash)",
        |capacity, pairs| {
            // Need to adjust the requested capacity of Rust's table
            let mut table = RawTableDoubleHash::with_capacity((capacity as f64 * 0.875) as usize);
            for (hash, index) in pairs {
                table.insert(*hash, (*hash, *index), |(hash, _)| *hash);
            }
            table
        },
        |hash, table| {
            let (value, probe_length, false_positives) = table.get_with_stats(hash, |(stored_hash, _)| *stored_hash == hash);
            let output = value.map(|(_, index)| index);
            (output, Stats { probe_length, false_positives })
        },
    ));
    results.push(test(
        "RawTable (extra)",
        |capacity, pairs| {
            // Need to adjust the requested capacity of Rust's table
            let mut table = RawTableExtraBit::with_capacity((capacity as f64 * 0.875) as usize);
            for (hash, index) in pairs {
                table.insert(*hash, (*hash, *index), |(hash, _)| *hash);
            }
            table
        },
        |hash, table| {
            let (value, probe_length, false_positives) = table.get_with_stats(hash, |(stored_hash, _)| *stored_hash == hash);
            let output = value.map(|(_, index)| index);
            (output, Stats { probe_length, false_positives })
        },
    ));
    results.push(test(
        "NewTable",
        |capacity, pairs| {
            // Need to adjust the requested capacity of Rust's table
            let mut table = FixedTable::new_with_capacity(capacity);
            for (hash, index) in pairs {
                table.write(*hash, *index).unwrap();
            }
            table.to_read_only()
        },
        |hash, table| {
            let (value, stats) = table.measure_get_stats(hash);
            (value, Stats { probe_length: stats.chunks_accessed, false_positives: stats.tag_false_positives })
        },
    ));
    results.push(test(
        "NewTable3 (g16)",
        |capacity, pairs| {
            // Need to adjust the requested capacity of Rust's table
            let mut table: WritableFixedTable<_, Group16> = WritableFixedTable::with_capacity(capacity);
            for (hash, index) in pairs {
                table.insert(*hash, *index).unwrap();
            }
            table.to_read_only()
        },
        |hash, table| {
            let (value, probe_length, false_positives) = table.get_with_stats(hash);
            (value, Stats { probe_length, false_positives })
        },
    ));
    results.push(test(
        "NewTable3 (g8)",
        |capacity, pairs| {
            // Need to adjust the requested capacity of Rust's table
            let mut table: WritableFixedTable<_, Group8> = WritableFixedTable::with_capacity(capacity);
            for (hash, index) in pairs {
                table.insert(*hash, *index).unwrap();
            }
            table.to_read_only()
        },
        |hash, table| {
            let (value, probe_length, false_positives) = table.get_with_stats(hash);
            (value, Stats { probe_length, false_positives })
        },
    ));

    results.push(test(
        "NewTable3 (g8 sw)",
        |capacity, pairs| {
            // Need to adjust the requested capacity of Rust's table
            let mut table: WritableFixedTable<_, GroupType8SwissTable> = WritableFixedTable::with_capacity(capacity);
            for (hash, index) in pairs {
                table.insert(*hash, *index).unwrap();
            }
            table.to_read_only()
        },
        |hash, table| {
            let (value, probe_length, false_positives) = table.get_with_stats(hash);
            (value, Stats { probe_length, false_positives })
        },
    ));

    results.push(test(
        "NewTable3 (g16 sw)",
        |capacity, pairs| {
            // Need to adjust the requested capacity of Rust's table
            let mut table: WritableFixedTable<_, Group16Swiss> = WritableFixedTable::with_capacity(capacity);
            for (hash, index) in pairs {
                table.insert(*hash, *index).unwrap();
            }
            table.to_read_only()
        },
        |hash, table| {
            let (value, probe_length, false_positives) = table.get_with_stats(hash);
            (value, Stats { probe_length, false_positives })
        },
    ));

    display(results);
}
