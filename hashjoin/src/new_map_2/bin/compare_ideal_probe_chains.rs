use std::cmp::{max, min};
use std::collections::HashSet;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

pub struct Stats {
    pub probe_length: usize,
    pub false_positives: usize,
}

fn test<M, L, T>(
    name: &str,
    capacity: usize,
    load_ratio: f64,
    mut make: M,
    mut lookup: L,
)
where
    M: FnMut(usize, &[u64]) -> T,
    L: FnMut(u64, &T) -> (bool, Stats),
{
    let value_count = (capacity as f64 * load_ratio) as usize;

    // Start with a correctness check
    let mut rng = StdRng::seed_from_u64(0);
    let mut hash_set = HashSet::new();
    let hashes: Vec<_> = std::iter::repeat_with(|| rng.gen::<u64>())
        .filter_map(|hash| -> Option<Result<u64, ()>> {
            if hash_set.len() >= value_count {
                return Some(Err(()))
            } else if hash_set.insert(hash) {
                Some(Ok(hash))
            } else {
                None
            }
        })
        .take_while(|result| result.is_ok())
        .map(|result| result.unwrap())
        .collect();
    let table = make(capacity, &hashes);
    for hash in &hashes {
        assert!(lookup(*hash, &table).0, "Could not find hash {} in table {}", hash, name);
    }

    const ITERATIONS: usize = 1000;

    let mut rng = StdRng::seed_from_u64(0);

    let mut total_probe_length = 0usize;
    let mut total_false_positives = 0usize;
    let mut total_miss_probe_length = 0usize;
    let mut total_miss_false_positives = 0usize;

    for _ in 0..ITERATIONS {
        let mut hashes = vec![0u64; value_count];
        rng.fill(&mut hashes[..]);
        let table = make(capacity, &hashes);

        for hash in &hashes {
            let stats = lookup(*hash, &table).1;
            total_probe_length += stats.probe_length;
            total_false_positives += stats.false_positives;
        }

        let hash_set = hashes.iter().cloned().collect::<HashSet<_>>();
        let miss_hashes: Vec<_> = std::iter::repeat_with(|| rng.gen::<u64>())
            .filter(|hash| !hash_set.contains(hash))
            .take(value_count)
            .collect();
        for hash in &miss_hashes {
            let stats = lookup(*hash, &table).1;
            total_miss_probe_length += stats.probe_length;
            total_miss_false_positives += stats.false_positives;
        }
    }

    let total_value_count = value_count * ITERATIONS;
    println!("{} (capacity {}, load ratio {}): ", name, capacity, load_ratio);
    println!("                         Hits        Misses");
    println!("  Avg probe length:      {:.5}      {:.5}", total_probe_length as f64 / total_value_count as f64, total_miss_probe_length as f64 / total_value_count as f64);
    println!("  Avg false positives:   {:.5}      {:.5}", total_false_positives as f64 / total_value_count as f64, total_miss_false_positives as f64 / total_value_count as f64);
}

pub fn main() {
    test_hybrid_sequence();

    let table_capacity = 16384;
    let load_ratio = 0.5;

    test(
        "SwissTable (G 8, T 8)",
        table_capacity,
        load_ratio,
        initialize_table::<8, 8, TriangleProbe<8>>,
        |hash, table| table.get(hash),
    );
    test(
        "SwissTable (G 8, T 7)",
        table_capacity,
        load_ratio,
        initialize_table::<8, 7, TriangleProbe<8>>,
        |hash, table| table.get(hash),
    );
    test(
        "FollyTable (G 8, T 8, OVERFLOW BIT)",
        table_capacity,
        load_ratio,
        initialize_table::<8, 8, FollySeq<8, true>>,
        |hash, table| table.get(hash),
    );
    test(
        "FollyTable (G 8, T 8)",
        table_capacity,
        load_ratio,
        initialize_table::<8, 8, FollySeq<8, false>>,
        |hash, table| table.get(hash),
    );
    test(
        "FollyTable (G 8, T 7)",
        table_capacity,
        load_ratio,
        initialize_table::<8, 7, FollySeq<8, false>>,
        |hash, table| table.get(hash),
    );
    test(
        "HybridTable (G 8, T 8)",
        table_capacity,
        load_ratio,
        initialize_table::<8, 8, Hybrid<8>>,
        |hash, table| table.get(hash),
    );
    test(
        "HybridTable (G 8, T 7)",
        table_capacity,
        load_ratio,
        initialize_table::<8, 7, Hybrid<8>>,
        |hash, table| table.get(hash),
    );
    test(
        "SwissTable (G 16, T 8)",
        table_capacity,
        load_ratio,
        initialize_table::<16, 8, TriangleProbe<16>>,
        |hash, table| table.get(hash),
    );
    test(
        "SwissTable (G 16, T 7)",
        table_capacity,
        load_ratio,
        initialize_table::<16, 7, TriangleProbe<16>>,
        |hash, table| table.get(hash),
    );
    test(
        "FollyTable (G 16, T 8, OVERFLOW BIT)",
        table_capacity,
        load_ratio,
        initialize_table::<16, 8, FollySeq<16, true>>,
        |hash, table| table.get(hash),
    );
    test(
        "FollyTable (G 16, T 8)",
        table_capacity,
        load_ratio,
        initialize_table::<16, 8, FollySeq<16, false>>,
        |hash, table| table.get(hash),
    );
    test(
        "FollyTable (G 16, T 7)",
        table_capacity,
        load_ratio,
        initialize_table::<16, 7, FollySeq<16, false>>,
        |hash, table| table.get(hash),
    );
    test(
        "HybridTable (G 16, T 8)",
        table_capacity,
        load_ratio,
        initialize_table::<16, 8, Hybrid<16>>,
        |hash, table| table.get(hash),
    );
    test(
        "HybridTable (G 16, T 7)",
        table_capacity,
        load_ratio,
        initialize_table::<16, 7, Hybrid<16>>,
        |hash, table| table.get(hash),
    );
}

fn initialize_table<const GROUP_SIZE: usize, const TAG_LENGTH: usize, P>(capacity: usize, hashes: &[u64]) -> Table<GROUP_SIZE, TAG_LENGTH, P>
where P: ProbeSeq
{
    let mut table = Table::<GROUP_SIZE, TAG_LENGTH, P>::new(capacity);
    assert_eq!(table.capacity(), capacity);
    for hash in hashes {
        table.insert(*hash);
    }
    table
}

struct Table<const GROUP_SIZE: usize, const TAG_LENGTH: usize, P> {
    tags: Box<[u8]>,
    buckets: Box<[u64]>,
    _phantom: std::marker::PhantomData<P>,
}

impl <const GROUP_SIZE: usize, const TAG_LENGTH: usize, P> Table<GROUP_SIZE, TAG_LENGTH, P>
where P: ProbeSeq
{
    pub fn new(capacity: usize) -> Self {
        assert_eq!(capacity / GROUP_SIZE * GROUP_SIZE, capacity, "Capacity must be a multiple of group size");
        assert!(capacity.is_power_of_two(), "Capacity must be a power of two");

        Self {
            tags: vec![0; capacity].into_boxed_slice(),
            buckets: vec![0; capacity].into_boxed_slice(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn capacity(&self) -> usize {
        self.tags.len()
    }

    pub fn insert(&mut self, hash: u64) -> bool {
        let search_tag = max((hash >> (64 - TAG_LENGTH)) as u8, 1);
        let stored_hash = (hash & (u64::MAX >> 2)) | (1 << 62);
        let (mut index, mut state) = P::start(hash, self.tags.len());
        let mut attempts = 0;

        loop {
            let group_start = index;
            let group_end = group_start + GROUP_SIZE;

            // Check all the tags in the group
            for i in group_start..group_end {
                let inner_index = i % self.tags.len();
                if self.tags[inner_index] == search_tag && (self.buckets[inner_index] & (u64::MAX >> 1)) == stored_hash {
                    // println!("Failed to insert hash {}, it already existed at index {}", hash, i);
                    return false;
                }
            }

            // Check for empty slots
            for i in group_start..group_end {
                let inner_index = i % self.tags.len();
                if self.tags[inner_index] == 0 && (self.buckets[inner_index] & (u64::MAX >> 1)) == 0 {
                    // println!("Inserting hash {} at index {}", hash, i);
                    self.tags[inner_index] = search_tag;
                    self.buckets[inner_index] = stored_hash;
                    return true;
                }
            }

            attempts += 1;
            if attempts >= (self.tags.len() / GROUP_SIZE) {
                // println!("Failed to insert hash {}, ran out of attempts {}", hash, attempts);
                return false;
            }

            // Probe to next group
            P::probed_past_group(&mut self.tags, &mut self.buckets, group_start, group_end);
            index = P::next(index, &mut state, self.tags.len());
        }
    }

    pub fn get(&self, hash: u64) -> (bool, Stats) {
        let search_tag = max((hash >> (64 - TAG_LENGTH)) as u8, 1);
        let stored_hash = (hash & (u64::MAX >> 2)) | (1 << 62);
        let (mut index, mut state) = P::start(hash, self.tags.len());
        let mut attempts = 0;

        let mut probe_length = 0;
        let mut false_positives = 0;

        // println!("Searching for hash {}, stored hash {}, search tag {}, start index {}", hash, stored_hash, search_tag, index);

        loop {
            let group_start = index;
            let group_end = group_start + GROUP_SIZE;

            // Check all the tags in the group
            for i in group_start..group_end {
                let inner_index = i % self.tags.len();
                let tag = self.tags[inner_index];
                if tag == search_tag {
                    if (self.buckets[inner_index] & (u64::MAX >> 1)) == stored_hash {
                        return (true, Stats { probe_length, false_positives });
                    } else {
                        false_positives += 1;
                    }
                }
            }

            attempts += 1;
            let mut tags = [0u8; GROUP_SIZE];
            let mut values = [0u64; GROUP_SIZE];
            for i in 0..GROUP_SIZE {
                tags[i] = self.tags[(i + group_start) % self.tags.len()];
                values[i] = self.buckets[(i + group_start) % self.tags.len()];
            }
            if P::can_stop(&tags, &values) || attempts >= (self.tags.len() / GROUP_SIZE) {
                return (false, Stats { probe_length, false_positives });
            }

            // Probe to next group
            index = P::next(index, &mut state, self.tags.len());
            probe_length += 1;
        }
    }
}

trait ProbeSeq {
    type S;

    fn start(hash: u64, table_capacity: usize) -> (usize, Self::S);
    fn next(previous: usize, state: &mut Self::S, table_capacity: usize) -> usize;
    fn probed_past_group(tags: &mut [u8], values: &mut [u64], group_start: usize, group_end: usize);
    fn can_stop(tags: &[u8], values: &[u64]) -> bool;
}

const OVERFLOW_BIT_MASK: u64 = 1 << 63;
struct FollySeq<const GROUP_SIZE: usize, const USE_OVERFLOW_BIT: bool>;

impl <const GROUP_SIZE: usize, const USE_OVERFLOW_BIT: bool> ProbeSeq for FollySeq<GROUP_SIZE, USE_OVERFLOW_BIT> {
    type S = usize;


    fn start(hash: u64, table_capacity: usize) -> (usize, Self::S) {
        let chunk_count = table_capacity / GROUP_SIZE;
        assert!(chunk_count.is_power_of_two(), "Chunk count must be a power of two");

        let top_8_bits = hash as usize >> (64 - 8);
        let stride = top_8_bits * 2 + 1;
        let chunk_index = hash as usize % chunk_count;
        (chunk_index * GROUP_SIZE, stride)
    }

    fn next(previous: usize, state: &mut Self::S, table_capacity: usize) -> usize {
        let chunk_count = table_capacity / GROUP_SIZE;
        assert!(chunk_count.is_power_of_two(), "Chunk count must be a power of two");

        let previous_chunk_index = previous / GROUP_SIZE;
        ((previous_chunk_index + *state) % chunk_count) * GROUP_SIZE
    }

    fn probed_past_group(tags: &mut [u8], values: &mut [u64], group_start: usize, group_end: usize) {
        if USE_OVERFLOW_BIT {
            // Set a bit in the last value to indicate that we needed to pass this group
            values[(group_end - 1) % values.len()] |= OVERFLOW_BIT_MASK;
        }
    }

    fn can_stop(tags: &[u8], values: &[u64]) -> bool {
        if USE_OVERFLOW_BIT {
            // Check if the overflow bit is set in the last value
            (values[values.len() - 1] & OVERFLOW_BIT_MASK) == 0
        } else {
            tags.iter().any(|tag| *tag == 0)
        }
    }
}

struct TriangleProbe<const GROUP_SIZE: usize>;

impl <const GROUP_SIZE: usize> ProbeSeq for TriangleProbe<GROUP_SIZE> {
    type S = usize;

    fn start(hash: u64, table_capacity: usize) -> (usize, Self::S) {
        let index = hash as usize % table_capacity;
        (index, 0)
    }

    fn next(previous: usize, stride: &mut Self::S, table_capacity: usize) -> usize {
        *stride += GROUP_SIZE;
        (previous + *stride) % table_capacity
    }

    fn probed_past_group(tags: &mut [u8], values: &mut [u64], group_start: usize, group_end: usize) {}

    fn can_stop(tags: &[u8], values: &[u64]) -> bool {
        tags.iter().any(|tag| *tag == 0)
    }
}

struct Hybrid<const GROUP_SIZE: usize>;

impl <const GROUP_SIZE: usize> ProbeSeq for Hybrid<GROUP_SIZE> {
    type S = usize;

    fn start(hash: u64, table_capacity: usize) -> (usize, Self::S) {
        let stride = (hash as usize >> 56) * 2 + 1;
        let index = hash as usize % table_capacity;
        (index, stride)
    }

    fn next(previous: usize, stride: &mut Self::S, table_capacity: usize) -> usize {
        (previous + (*stride * GROUP_SIZE)) % table_capacity
    }

    fn probed_past_group(tags: &mut [u8], values: &mut [u64], group_start: usize, group_end: usize) {}

    fn can_stop(tags: &[u8], values: &[u64]) -> bool {
        tags.iter().any(|tag| *tag == 0)
    }
}

fn test_hybrid_sequence() {
    const GROUP_SIZE: usize = 8;
    let mut rng = StdRng::seed_from_u64(0);

    let table_capacities: Vec<_> = (3..16).map(|p| 2usize.pow(p)).collect();
    let hashes: Vec<_> = (0..10000).map(|_| rng.gen::<u64>()).collect();

    for table_capacity in table_capacities {
        let chunks = table_capacity / GROUP_SIZE;
        for hash in &hashes {
            let mut covered_indices = vec![false; table_capacity];
            let (mut index, mut state) = Hybrid::<GROUP_SIZE>::start(*hash, table_capacity);
            for _ in 0..chunks {
                for offset in 0..GROUP_SIZE {
                    // assert!(!covered_indices[index], "Expected index {} not to be visited yet", index);
                    covered_indices[(index + offset) % table_capacity] = true;
                }

                index = Hybrid::<GROUP_SIZE>::next(index, &mut state, table_capacity);
            }

            for (index, covered) in covered_indices.iter().enumerate() {
                assert!(covered, "Expected all indices to be visited, index {} was not. Table size {}, State {:?}", index, table_capacity, state);
            }
        }
    }
}

/*
step size = 3, table size = 8
s, current index
0, 0
3, 3
6, 1
9, 2
12, 6
15, 5
18, 7

step size = 375, table size = 16
current index
0
7
5
10
6
9
3
11
3
 */
