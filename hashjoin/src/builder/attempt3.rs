use crate::builder::global_index_tracker::{GlobalIndexTracker, Offset};
use crate::builder::utils::{get_owned_range, ClaimOnce, UnsafeCellSendWrapper};
use hashbrown::hash_table::Entry;
use hashbrown::raw::{capacity_to_buckets, ProbeSeq};
use hashbrown::HashTable;
use std::cell::UnsafeCell;
use std::cmp::max;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::vec::Vec;
use tokio::sync::{broadcast, Barrier, BarrierWaitResult};
use tokio::sync::broadcast::error::RecvError;
use crate::lookup::Lookup;

#[derive(Debug, Clone, PartialEq)]
struct OrderedOverflowBuffer {
    global_index: usize,
    global_offset: usize,
    buffer: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LocalEntry {
    transformed_hash: u64,
    hash: u64,
    global_index: usize,
    end_of_chain: usize,
}

// #[repr(transparent)]
// struct LocalPartitionIndex(usize);
//
// impl LocalPartitionIndex {
//     fn new_from_hash_with_shift(hash: u64, shift: u32) -> Self {
//         // Leave the high 7 bits for the HashBrown SIMD tag. This has the effect of using the
//         // first n bits after 7 as the table index.
//         let index = ((hash << 7) >> shift) as usize;
//         Self(index)
//     }
// }
//
// #[repr(transparent)]
// struct InsertionIndex(usize);
//
// impl InsertionIndex {
//     fn new(global_offset: usize, local_index: usize) -> Self {
//         Self(global_offset + local_index + 1)
//     }
// }

// Sets the first 7 bits of an u64 to 0
// const EXCLUDE_SIMD_TAG_MASK: u64 = (1 << (64 - 7)) - 1;

struct LocalTablePartitioner {
    tag_bits_mask: u64,
    partition_bits_mask: usize,
    partition_bits_count: u32,
    remaining_bits_count: u32,
    reverse_bits_shift: u32,
}

impl LocalTablePartitioner {
    fn new(table_partitions: usize) -> Self {
        assert!(table_partitions > 1);
        let tag_bits_mask = u64::MAX >> (64 - 7) << (64 - 7);
        let partition_bits_mask = table_partitions - 1;
        let partition_bits_count = 64 - partition_bits_mask.leading_zeros();
        let remaining_bits_count = 64 - 7 - partition_bits_count;
        let reverse_bits_shift = partition_bits_mask.leading_zeros();
        Self {
            tag_bits_mask,
            partition_bits_mask,
            partition_bits_count,
            remaining_bits_count,
            reverse_bits_shift,
        }
    }

    // Each local table is partitioned by the lowest n bits of the hash
    fn get_local_table_partition(&self, hash: u64) -> usize {
        match (hash as usize & self.partition_bits_mask).reverse_bits().checked_shr(self.reverse_bits_shift) {
            Some(x) => x,
            None => panic!("Failed to calculate table partition. hash: {}, self.mask: {}, self.shift: {}", hash, self.partition_bits_mask, self.reverse_bits_shift),
        }
    }

    // Performs the reverse operation of the above. Obviously this is a lossy transformation since
    // we cannot undo the mask, but for the purposes of this algorithm, the unmasked bits are
    // ignored.
    fn get_masked_hash_in_partition(&self, partition_index: usize) -> usize {
        (partition_index << self.reverse_bits_shift).reverse_bits()
    }

    fn get_transformed_hash(&self, hash: u64) -> u64 {
        // Leave the high 7 bits for the HashBrown SIMD tag
        let tag = hash & self.tag_bits_mask;
        // The partition bits are shifted to be the highest bits after the tag
        let partition_bits = (hash & self.partition_bits_mask as u64) << self.remaining_bits_count;
        // The originally unused bits become the lowest bits
        let remaining_bits = (hash & !self.tag_bits_mask) >> self.partition_bits_count;
        tag | partition_bits | remaining_bits
    }
}

#[cfg(test)]
mod local_table_partitioner_tests {
    use rand::{RngCore, SeedableRng};
    use crate::builder::attempt3::LocalTablePartitioner;

    #[test]
    pub fn applies_mask_and_reverses_bits() {
        let partitioner = LocalTablePartitioner::new(0b1000);
        assert_eq!(partitioner.get_local_table_partition(0b001), 0b100);
        assert_eq!(partitioner.get_local_table_partition(0b1100101001), 0b100);
        assert_eq!(partitioner.get_local_table_partition(0b1011), 0b110);
    }

    #[test]
    pub fn is_safe() {
        let partitioner = LocalTablePartitioner::new(4);
        for i in 1..=6 {
            partitioner.get_local_table_partition(i);
        }
    }

    #[test]
    pub fn extracts_original_hash_from_partition() {
        let partitioner = LocalTablePartitioner::new(0b1000);
        assert_eq!(partitioner.get_masked_hash_in_partition(0b001), 0b100);
        assert_eq!(partitioner.get_masked_hash_in_partition(0b100), 0b001);
        assert_eq!(partitioner.get_masked_hash_in_partition(0b110), 0b011);
    }

    #[test]
    pub fn transforms_hash() {
        let partitioner = LocalTablePartitioner::new(0b100000);

        // A single hard coded test. Top 7 bits are unchanged. Lower 5 bits should move to the top
        let original = 0b10110010_00000000_11111111_00000000_00000000_00000000_00000000_00010111;
        let new_bits = 0b10110011_01110000_00000111_11111000_00000000_00000000_00000000_00000000;
        assert_eq!(
            partitioner.get_transformed_hash(original),
            new_bits,
            "Original:    {:#064b}\nTransformed: {:#064b}\nExpected:    {:#064b}",
            original,
            partitioner.get_transformed_hash(original),
            new_bits,
        );

        // Now some randomly generated tests
        let mut rng = rand::rngs::StdRng::seed_from_u64(1234);
        let random_hashes = (0..16).map(|_| rng.next_u64()).collect::<Vec<_>>();
        for hash in random_hashes {
            // Mask of the top 7 bits which are the SIMD tag
            let tag_mask = u64::MAX >> (64 - 7) << (64 - 7);
            // Mask of the bottom 5 bits which are part of the partitioning scheme
            let partition_mask = 0b11111;

            let transformed_hash = partitioner.get_transformed_hash(hash);
            // Top 7 bits should be the same
            assert_eq!(transformed_hash & tag_mask, hash & tag_mask);
            // Next bits after the top 7 should be the same as the original lowest bits
            assert_eq!(transformed_hash >> (64 - 7 - 5) & partition_mask, hash & partition_mask);
            // Lowest transformed bits should be the same as the original non partition bits
            assert_eq!(transformed_hash << 7 << 5 >> 5 >> 7, hash << 7 >> 7 >> 5);
        }
    }
}

fn get_insertion_index(global_offset: usize, local_index: usize) -> usize {
    global_offset + local_index + 1
}

#[cfg(test)]
mod get_insertion_index_tests {
    use crate::builder::attempt3::get_insertion_index;

    #[test]
    pub fn test_get_insertion_index() {
        assert_eq!(get_insertion_index(10, 10), 21);
    }
}

fn locally_accumulate_input(
    global_offset: Offset,
    input: Vec<u64>,
    partitioner: &LocalTablePartitioner,
    tables: &mut [HashTable<LocalEntry>],
    // tablesi: impl IndexMut<LocalPartitionIndex, Output=RawTable<(u64, usize)>>,
    overflow: &mut [usize],
) {
    for (local_index, hash) in input.into_iter().enumerate() {
        let transformed_hash = partitioner.get_transformed_hash(hash);
        let insertion_index = get_insertion_index(global_offset.size, local_index);
        let table_index = partitioner.get_local_table_partition(hash);
        let table = &mut tables[table_index];

        // println!("Locally inserting {} at index {}", hash, insertion_index);
        match table.entry(
            transformed_hash,
            |item| item.transformed_hash == transformed_hash,
            |item| item.transformed_hash,
        ) {
            Entry::Occupied(mut occupied) => {
                let previous = std::mem::replace(&mut occupied.get_mut().global_index, insertion_index);
                // The individual overflow buffers are indexed by the local index, but they store
                // the insertion index
                overflow[local_index] = previous;
            }
            Entry::Vacant(vacant) => {
                vacant.insert(LocalEntry {
                    transformed_hash,
                    hash,
                    global_index: insertion_index,
                    end_of_chain: insertion_index,
                });
            }
        }
    }
}

async fn accumulate_locally(
    table_count: usize,
    global_index_tracker: &GlobalIndexTracker,
    inputs: Vec<Vec<u64>>,
) -> (Vec<HashTable<LocalEntry>>, Vec<OrderedOverflowBuffer>) {
    // Chosen arbitrarily, based on the value in DashMap
    // let scale_factor = 4;
    // let table_count = (parallelism * scale_factor).next_power_of_two();
    let partitioner = LocalTablePartitioner::new(table_count);

    let mut tables = vec![HashTable::<LocalEntry>::with_capacity(0); table_count];
    let mut overflows = vec![];
    for input in inputs {
        let global_offset = global_index_tracker.allocate(input.len()).await;
        let mut overflow = vec![0usize; input.len()];
        locally_accumulate_input(
            global_offset,
            input,
            &partitioner,
            &mut tables,
            &mut overflow,
        );

        overflows.push(OrderedOverflowBuffer {
            global_offset: global_offset.size,
            global_index: global_offset.index,
            buffer: overflow,
        });
    }

    (tables, overflows)
}

#[cfg(test)]
mod accumulate_locally_tests {
    use crate::builder::attempt3::{accumulate_locally, LocalEntry, LocalTablePartitioner, OrderedOverflowBuffer};
    use crate::builder::global_index_tracker::GlobalIndexTracker;

    #[tokio::test]
    pub async fn hashes_are_partitioned() {
        let global_index_tracker = GlobalIndexTracker::new();
        let (tables, overflows) = accumulate_locally(
            4,
            &global_index_tracker,
            vec![vec![1, 2, 3], vec![4, 5, 6]],
        ).await;

        let partitioner = LocalTablePartitioner::new(4);
        for i in 1u64..=6 {
            assert_eq!(
                tables[partitioner.get_local_table_partition(i)]
                    .find(i, |item| item.hash == i)
                    .cloned(),
                Some(LocalEntry {
                    transformed_hash: partitioner.get_transformed_hash(i),
                    hash: i,
                    global_index: i as usize,
                    end_of_chain: i as usize,
                }),
            );
        }
    }

    #[tokio::test]
    pub async fn each_input_creates_an_overflow() {
        let global_index_tracker = GlobalIndexTracker::new();
        let (tables, overflows) = accumulate_locally(
            4,
            &global_index_tracker,
            vec![vec![1, 2, 3], vec![4, 5, 6]],
        ).await;

        assert_eq!(overflows, vec![
            OrderedOverflowBuffer {
                global_index: 0,
                global_offset: 0,
                buffer: vec![0; 3],
            },
            OrderedOverflowBuffer {
                global_index: 1,
                global_offset: 3,
                buffer: vec![0; 3]
            },
        ]);
    }

    #[tokio::test]
    pub async fn duplicate_hashes_are_written_to_the_overflows() {
        let global_index_tracker = GlobalIndexTracker::new();
        let (tables, overflows) = accumulate_locally(
            4,
            &global_index_tracker,
            vec![vec![1, 2, 1], vec![1, 5, 2]],
        ).await;

        assert_eq!(overflows, vec![
            OrderedOverflowBuffer {
                global_index: 0,
                global_offset: 0,
                buffer: vec![0, 0, 1],
            },
            OrderedOverflowBuffer {
                global_index: 1,
                global_offset: 3,
                buffer: vec![3, 0, 2]
            },
        ]);
    }
}

async fn share_tables(
    parallelism: usize,
    local_partitioned_tables: Vec<HashTable<LocalEntry>>,
    partitioned_tables_sender: broadcast::Sender<Arc<Vec<HashTable<LocalEntry>>>>,
    mut partitioned_tables_receiver: broadcast::Receiver<Arc<Vec<HashTable<LocalEntry>>>>,
) -> Vec<Arc<Vec<HashTable<LocalEntry>>>> {
    let expected_partitions = local_partitioned_tables.len();

    partitioned_tables_sender.send(Arc::new(local_partitioned_tables))
        .map_err(|_| "Failed to share partitioned tables")
        .unwrap();
    drop(partitioned_tables_sender);

    let mut all_tables = Vec::with_capacity(parallelism);
    loop {
        match partitioned_tables_receiver.recv().await {
            Ok(tables) => {
                assert_eq!(tables.len(), expected_partitions);
                all_tables.push(tables)
            },
            Err(RecvError::Closed) => {
                assert_eq!(all_tables.len(), parallelism);
                break;
            }
            Err(RecvError::Lagged(_)) => {
                panic!("Receiver lagged while waiting for partitioned tables");
            }
        }
    }

    all_tables
}

async fn allocate_global_overflow_buffer(
    global_index_tracker: &GlobalIndexTracker,
    overflow_buffer_sender: Arc<ClaimOnce<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>>,
    mut overflow_buffer_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>,
) -> Arc<UnsafeCellSendWrapper<Vec<usize>>> {
    if let Ok(overflow_buffer_sender) = overflow_buffer_sender.claim() {
        let current_offset = global_index_tracker.try_get_current_offset()
            .expect("Failed to get current offset from tracker. Something else must be using it");

        // The overflow buffer always needs to be 1 larger than the size of the input
        let overflow_buffer = vec![0; current_offset.size + 1];
        let shared_overflow_buffer = Arc::new(UnsafeCellSendWrapper::new(UnsafeCell::new(overflow_buffer)));
        overflow_buffer_sender.send(shared_overflow_buffer.clone())
            .map_err(|_| "Failed to send shared overflow buffer")
            .unwrap();
        shared_overflow_buffer
    } else {
        overflow_buffer_receiver.recv()
            .await
            .map_err(|err| format!("Failed to receive shared overflow buffer. Err: {:?}", err))
            .unwrap()
    }
}

fn write_local_overflow_buffers_to_global_buffer(
    local_overflow_buffers: Vec<OrderedOverflowBuffer>,
    mutable_shared_overflow_buffer: &mut Vec<usize>
) {
    for OrderedOverflowBuffer { global_offset, buffer, .. } in local_overflow_buffers {
        // We always leave the first element of the buffer empty
        let start = global_offset + 1;
        let end = start + buffer.len();
        mutable_shared_overflow_buffer[start..end].copy_from_slice(&buffer);
    }
}

async fn allocate_shared_bucket_counts(
    local_bucket_count: usize,
    estimated_total_size: usize,
    bucket_counts_sender: Arc<ClaimOnce<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>>,
    mut bucket_counts_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>,
) -> Arc<UnsafeCellSendWrapper<Vec<usize>>> {
    if let Ok(bucket_counts_sender) = bucket_counts_sender.claim() {
        let estimated_bucket_count = max(
            local_bucket_count,
            capacity_to_buckets(estimated_total_size)
                .expect("Could not compute estimated buckets required due to an overflow"),
        );
        assert!(estimated_bucket_count.is_power_of_two());

        let bucket_contents = vec![0; estimated_bucket_count];
        let shared_bucket_contents = Arc::new(UnsafeCellSendWrapper::new(UnsafeCell::new(bucket_contents)));
        bucket_counts_sender.send(shared_bucket_contents.clone())
            .map_err(|_| "Failed to send shared bucket counts vector")
            .unwrap();
        shared_bucket_contents
    } else {
        bucket_counts_receiver.recv()
            .await
            .map_err(|err| format!("Failed to receive shared bucket counts vector. Err: {:?}", err))
            .unwrap()
    }
}

fn merge_owned_partitions_and_update_counts(
    parallelism: usize,
    thread_index: usize,
    local_bucket_count: usize,
    all_tables: Vec<Arc<Vec<HashTable<LocalEntry>>>>,
    shared_overflow_buffer: &mut Vec<usize>,
    shared_bucket_counts: &mut Vec<usize>,
) -> Vec<HashTable<LocalEntry>> {
    let partitioner = LocalTablePartitioner::new(shared_bucket_counts.len());
    let (low, high) = get_owned_range(local_bucket_count, parallelism, thread_index);

    (low..high).into_iter()
        .map(|partition_index| {
            let partitions = all_tables.iter()
                .map(|tables| &tables[partition_index])
                .collect::<Vec<_>>();
            // println!("Counting values in partition. Thread index: {}, low high: {:?}, partitions: {:?}", thread_index, (low, high), partitions.iter().map(|x|x.len()).collect::<Vec<_>>());

            // Clone the first table to avoid having to merge it with nothing
            let mut merged_table = partitions[0].clone();
            for item in merged_table.iter() {
                // Update the count for this particular hash
                let counter_index = partitioner.get_local_table_partition(item.hash);
                shared_bucket_counts[counter_index] += 1;
            }

            // Merge each of the other partitions into the first table. Duplicate entries are
            // written to the overflow buffer.
            for bucket in partitions.into_iter().skip(1) {
                for item in bucket {
                    match merged_table.entry(
                        item.transformed_hash,
                        |other| other.transformed_hash == item.transformed_hash,
                        |other| other.transformed_hash,
                    ) {
                        Entry::Occupied(mut entry) => {
                            let existing = std::mem::replace(&mut entry.get_mut().global_index, item.global_index);
                            shared_overflow_buffer[item.end_of_chain] = existing; // WRONG!
                        },
                        Entry::Vacant(entry) => {
                            entry.insert(*item);

                            // Update the count for this particular hash
                            let counter_index = partitioner.get_local_table_partition(item.hash);
                            shared_bucket_counts[counter_index] += 1;
                        },
                    }
                }
            }

            merged_table
        })
        .collect::<Vec<_>>()
}

async fn allocate_destination_table(
    destination_table_sender: Arc<ClaimOnce<(broadcast::Sender<usize>, broadcast::Sender<Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>>)>>,
    mut destination_table_size_receiver: broadcast::Receiver<usize>,
    mut destination_table_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>>,
    all_cells_merged_written_future: impl Future<Output=BarrierWaitResult> + Sized,
    shared_bucket_counts: &Vec<usize>,
) -> (Pin<Box<dyn Future<Output=usize> + Send + 'static>>, Pin<Box<dyn Future<Output=Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>> + Send + 'static>>) {
    if let Ok((destination_table_size_sender, destination_table_sender)) = destination_table_sender.claim() {
        // Now wait until all the data is valid
        all_cells_merged_written_future.await;

        // Calculate and share the total size so other threads can start other work
        let total_size = shared_bucket_counts.iter().sum::<usize>();
        let buckets = capacity_to_buckets(total_size)
            .expect("Could not compute buckets required due to an overflow");
        // The number of buckets in the final table, cannot be greater than the number of count
        // buckets.
        assert!(buckets <= shared_bucket_counts.len());

        // We assume 8 entries per bucket
        destination_table_size_sender.send(buckets)
            .map_err(|_| "Failed to send total size")
            .unwrap();

        // Allocate the final table
        let final_table = HashTable::with_capacity(total_size);
        assert_eq!(final_table.bucket_count(), buckets);
        // println!("Final table allocated. Buckets {}, bucket mask {:#b}", buckets, buckets - 1);

        let shared_final_table = Arc::new(UnsafeCellSendWrapper::new(UnsafeCell::new(final_table)));
        destination_table_sender.send(shared_final_table.clone())
            .map_err(|_| "Failed to send shared final table")
            .unwrap();
        drop(destination_table_sender);
        drop(destination_table_receiver);

        (Box::pin(std::future::ready(buckets)), Box::pin(std::future::ready(shared_final_table)))
    } else {
        // Now wait until all the data is valid
        all_cells_merged_written_future.await;

        (
            Box::pin(async move {
                destination_table_size_receiver.recv()
                    .await
                    .map_err(|err| format!("Failed to receive total size. Err: {:?}", err))
                    .unwrap()
            }),
            Box::pin(async move {
                destination_table_receiver.recv()
                    .await
                    .map_err(|err| format!("Failed to receive shared final table. Err: {:?}", err))
                    .unwrap()
            }),
        )
    }
}

fn pretend_to_write_hashes_to_map<F>(
    partitioner: &LocalTablePartitioner,
    table_bucket_mask: usize,
    bucket_index: usize,
    mut count: usize,
    occupied: &mut Vec<bool>,
    mut write_callback: F,
)
    where F: FnMut(usize)
{
    if count == 0 {
        return;
    }

    let example_masked_hash = partitioner.get_masked_hash_in_partition(bucket_index);

    // Since we know the number of buckets in the final table is less or equal to the number of
    // buckets in the counter vector, every hash in this bucket will follow the same probe seq.
    // println!("Creating probe seq. Bucket index {}, example hash {}, table_bucket_mask {:#b}", bucket_index, example_masked_hash, table_bucket_mask);
    let mut probe_seq = ProbeSeq {
        pos: example_masked_hash & table_bucket_mask,
        stride: 0,
    };

    // Write count values to the occupied vec, following the ProbeSeq
    loop {
        for group_pos in 0usize..8 {
            let index = (probe_seq.pos + group_pos) & table_bucket_mask;
            if !std::mem::replace(&mut occupied[index], true) {
                count -= 1;

                // println!("Pretending to write hash {} to index {} (probe seq {}, bit {}) using probing hash {}", example_masked_hash, index, probe_seq.pos, group_pos, example_masked_hash & table_bucket_mask);

                write_callback(index);

                if count == 0 {
                    return;
                }
            } else {
                // println!("Skipping ____________ bucket {} to position {} (probe seq {}, bit {}) using probing hash {}", bucket_index, index, probe_seq.pos, group_pos, example_masked_hash & table_bucket_mask);
            }
        }

        probe_seq.move_next(table_bucket_mask);
    }
}

fn actually_write_hashes_to_map(
    table_bucket_mask: usize,
    hash: u64,
    value: usize,
    occupied: &mut Vec<bool>,
    mutable_destination_table: &mut HashTable<(u64, usize)>
) {
    // Since we know the number of buckets in the final table is less or equal to the number of
    // buckets in the counter vector, every hash in this bucket will follow the same probe seq.
    // println!("Creating probe seq. Bucket index {}, actual hash {}, table_bucket_mask {:#b}", bucket_index, hash, table_bucket_mask);
    let mut probe_seq = ProbeSeq {
        pos: hash as usize & table_bucket_mask,
        stride: 0,
    };

    // Write count values to the occupied vec, following the ProbeSeq
    loop {
        for group_pos in 0usize..8 {
            let index = (probe_seq.pos + group_pos) & table_bucket_mask;
            if !std::mem::replace(&mut occupied[index], true) {
                // println!("Actually writing hash {} to index {} (probe seq {}, bit {}) using probing hash {}", hash, index, probe_seq.pos, group_pos, hash as usize & table_bucket_mask);
                mutable_destination_table.insert_directly(hash, (hash, value), index);
                return;
            } else {
                // println!("Skipping ____________ bucket {} to position {} (probe seq {}, bit {}) using probing hash {}", bucket_index, index, probe_seq.pos, group_pos, hash as usize & table_bucket_mask);
            }
        }

        probe_seq.move_next(table_bucket_mask);
    }
}

#[cfg(test)]
mod write_to_map_tests {
    use hashbrown::HashTable;
    use crate::builder::attempt3::{pretend_to_write_hashes_to_map, LocalTablePartitioner};

    #[test]
    pub fn writes_to_correct_index_in_empty_map() {
        for value in 0..128 {
            // Imagine we are writing to a table with 12 entries
            let mut map = HashTable::with_capacity(12);
            let buckets = map.bucket_count();
            assert!(buckets.is_power_of_two());
            let map_mask = buckets - 1;

            // The map is currently completely empty
            let mut occupied = vec![false; buckets];

            // The partitioner has more partitions than the map
            let partitioner = LocalTablePartitioner::new(buckets << 1);

            // The value we are going to write is 23
            let partition_index = partitioner.get_local_table_partition(value);

            let mut called = false;
            pretend_to_write_hashes_to_map(
                &partitioner,
                map_mask,
                partition_index,
                1,
                &mut occupied,
                |index| {
                    called = true;
                    // The index we write to must be the exact same index that the map uses
                    let mut map = map.clone();
                    map.insert_unique(value, value, |v| *v);
                    let expected_index = map.find_index(value, |_| true).0.unwrap();
                    assert_eq!(index, expected_index);
                },
            );
            assert!(called);
        }
    }
}

async fn write_to_destination_table(
    parallelism: usize,
    thread_index: usize,
    local_bucket_count: usize,
    shared_bucket_counts: &Vec<usize>,
    local_merged_buckets: Vec<HashTable<LocalEntry>>,
    destination_table_size_future: impl Future<Output=usize> + Sized + Sized,
    shared_destination_table_future: impl Future<Output=Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>> + Sized + Sized
) -> Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>> {
    // 4. All threads will then allocate a local Vec<bool> of the same capacity as the table to
    //    track which table cells have been occupied. Then iterate over the cell counts vec and
    //    update the occupied boolean based on the ProbeSeq of the hash. If the thread "owns" a
    //    part of the range, it will also write directly to the final table
    let destination_table_capacity = destination_table_size_future.await;
    let mut occupied = vec![false; destination_table_capacity];

    let destination_table = shared_destination_table_future.await;
    let mutable_destination_table = unsafe { &mut *destination_table.get() };


    // 5. Each thread will iterate over all the counts and pretend to write those items to the final
    //    table. The local occupied vec is used to track which cells would have been written to by
    //    another thread, avoiding any possibility of races between threads. The thread will only
    //    actually write to the table when it is the owner for that particular range.
    let (low_bucket, high_bucket) = get_owned_range(local_bucket_count, parallelism, thread_index);

    // The number of local buckets, and the number of buckets counters are always powers of two,
    // and there are always more counter buckets. So to determine which counter buckets this thread
    // will own, we can just shift left the low and high bucket indices by the difference in the
    // power of two between the two lengths.
    assert!(local_bucket_count.is_power_of_two());
    assert!(shared_bucket_counts.len().is_power_of_two());
    assert!(local_bucket_count < shared_bucket_counts.len());
    let power_diff = shared_bucket_counts.len().trailing_zeros() - local_bucket_count.trailing_zeros();
    let low_owned_counter_index = low_bucket << (power_diff);
    let high_owned_counter_index = high_bucket << (power_diff);

    // println!(
    //     "Writing to destination table. Power diff: {}, all counts: {:?}, owned range: {:?}, owned_counts: {:?}",
    //     power_diff,
    //     shared_bucket_counts,
    //     (low_owned_counter_index, high_owned_counter_index),
    //     local_merged_buckets.iter().map(|x| x.len()).collect::<Vec<_>>(),
    // );

    let partitioner = LocalTablePartitioner::new(shared_bucket_counts.len());
    let table_bucket_mask = destination_table_capacity - 1;

    // Start with the low unowned buckets
    for bucket_index in 0..low_owned_counter_index {
        pretend_to_write_hashes_to_map(
            &partitioner,
            table_bucket_mask,
            bucket_index,
            shared_bucket_counts[bucket_index],
            &mut occupied,
            |_| {},
        );
    }

    // Then the owned buckets
    for local_owned_bucket in local_merged_buckets.iter() {
        for item in local_owned_bucket.iter() {
            actually_write_hashes_to_map(
                table_bucket_mask,
                item.hash,
                item.global_index,
                &mut occupied,
                mutable_destination_table,
            );
        }
    }
    // for bucket_index in low_owned_counter_index..high_owned_counter_index {
    //     let local_bucket_index = (bucket_index - low_owned_counter_index) >> power_diff;
    //     for item in local_merged_buckets[local_bucket_index].iter() {
    //         actually_write_hashes_to_map(
    //             table_bucket_mask,
    //             bucket_index,
    //             item.hash,
    //             item.global_index,
    //             &mut occupied,
    //             mutable_destination_table,
    //         );
    //     }
    // }

    // Then the high unowned buckets
    for bucket_index in high_owned_counter_index..shared_bucket_counts.len() {
        pretend_to_write_hashes_to_map(
            &partitioner,
            table_bucket_mask,
            bucket_index,
            shared_bucket_counts[bucket_index],
            &mut occupied,
            |_| {},
        );
    }

    destination_table
}

async fn merge_cells_cooperatively(
    parallelism: usize,
    thread_index: usize,
    global_index_tracker: &GlobalIndexTracker,
    local_bucket_count: usize,
    // Outer vec has length of `parallelism`, all inner vecs have length of the same power of two
    all_tables: Vec<Arc<Vec<HashTable<LocalEntry>>>>,
    // all_thread_sizes: Vec<usize>,
    local_overflow_buffers: Vec<OrderedOverflowBuffer>,
    // Capacity=1
    overflow_buffer_sender: Arc<ClaimOnce<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>>,
    overflow_buffer_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>,
    wait_for_all_overflow_buffers_written: Arc<Barrier>,
    // Capacity=1
    bucket_counts_sender: Arc<ClaimOnce<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>>,
    bucket_counts_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>,
    wait_for_all_cells_merged_written: Arc<Barrier>,
    destination_table_sender: Arc<ClaimOnce<(
        // Capacity=1
        broadcast::Sender<usize>,
        // Capacity=1
        broadcast::Sender<Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>>,
    )>>,
    destination_table_size_receiver: broadcast::Receiver<usize>,
    destination_table_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>>,
) -> (Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>, Arc<UnsafeCellSendWrapper<Vec<usize>>>) {
    // 1. Allocate the final overflow table based on the current size of the global index tracker
    let shared_overflow_buffer = allocate_global_overflow_buffer(
        global_index_tracker,
        overflow_buffer_sender,
        overflow_buffer_receiver,
    ).await;
    // println!("Allocated shared overflow buffer");

    // Write each of the overflow buffers to the shared global buffer
    // SAFETY: Each thread only writes to the part of the buffer where its local buffers should be
    // stored, which should never overlap with another thread's local buffer thanks to the global
    // offset tracker
    let mut mutable_shared_overflow_buffer = unsafe { &mut *shared_overflow_buffer.get() };
    write_local_overflow_buffers_to_global_buffer(local_overflow_buffers, &mut mutable_shared_overflow_buffer);
    // Signal that we have finished writing our overflow buffers, but don't wait for anyone else yet
    let all_overflow_buffers_written_future = wait_for_all_overflow_buffers_written.wait();
    // println!("Wrote to shared overflow buffer");

    // TODO this could be done at the same time as the first step
    // 2. Sum the total number of items in all tables. This is an overestimate of the total number
    //    of entries that will be in the final hash table as we haven't eliminated duplicates yet.
    // 0. Determine the number of buckets that a final hash table would use if it needed to store this
    //    many items. Allocate a Vec<usize> of length max(local_bucket_count, est_final_cell_count).
    //      local_bucket_count was based on (parallelism * scale_factor).next_power_of_two().
    //      est_final_cell_count is est_total_count.next_power_of_two(). Therefore, the length of
    //      the vec is always a power of two.
    let total_table_size = all_tables.iter()
        .flat_map(|x| x.iter().map(|y| y.len()))
        .sum::<usize>();
    let shared_bucket_counts = allocate_shared_bucket_counts(
        local_bucket_count,
        total_table_size,
        bucket_counts_sender,
        bucket_counts_receiver,
    ).await;
    // println!("Allocated shared bucket counts");

    // Now wait until all other threads have finished writing their overflow buffers, since we are
    // just about to use them
    all_overflow_buffers_written_future.await;
    // println!("All overflow buffers written");

    // 1. Merge the cells of each table that are available into a single table each. For each unique
    //    entry, increment the count in the Vec<usize> by 1 (each thread is guaranteed to touch
    //    independent counts). Each thread writes to a counter based on the lowest n bits of the
    //    hash. Naively, each thread would be writing to many different counters spread over the
    //    vec. If the lowest n bits were reversed before writing, then each thread would be writing
    //    to a contiguous set of counters, increasing cache efficiency.
    let mutable_shared_bucket_counts = unsafe { &mut *shared_bucket_counts.get() };
    let local_merged_buckets = merge_owned_partitions_and_update_counts(
        parallelism,
        thread_index,
        local_bucket_count,
        all_tables,
        mutable_shared_overflow_buffer,
        mutable_shared_bucket_counts,
    );
    // Signal that we have finished counting the entries in each bucket but don't wait for others yet.
    let all_cells_merged_written_future = wait_for_all_cells_merged_written.wait();
    // println!("Finished merging partitions");

    // Try to claim the destination table sender if we finished writing our buckets first as there
    // is probably less contention now than after waiting for the barrier
    let (destination_table_size_future, shared_destination_table_future) = allocate_destination_table(
        destination_table_sender,
        destination_table_size_receiver,
        destination_table_receiver,
        all_cells_merged_written_future,
        mutable_shared_bucket_counts,
    ).await;
    // println!("Allocated destination table");

    let destination_table = write_to_destination_table(
        parallelism,
        thread_index,
        local_bucket_count,
        mutable_shared_bucket_counts,
        local_merged_buckets,
        destination_table_size_future,
        shared_destination_table_future,
    ).await;
    // println!("Wrote to destination table");

    (destination_table, shared_overflow_buffer)

    /*
       4. All threads will then allocate a local Vec<bool> of the same capacity as the table to
          track which table cells have been occupied. Then iterate over the cell counts vec and
          update the occupied boolean based on the ProbeSeq of the hash. If the thread "owns" a
          part of the range, it will also write directly to the final table
       ------
       4. Each thread will iterate over the tables it has constructed and write the number of hashes
          that will fall into each destination bucket to the Vec.
            - Local accumulation, hashes are partitioned into buckets based on the n lowest bits of
              the hash.
                0b0011 1011 0110 0101
                                 ^^^^
              Then, based on the final size of the global table, a different n lower bits are used:
                0b0011 1011 0110 0101
                              ^^ ^^^^
              If the size of the final table is >= the number of buckets in the lower table, then
              each thread is guaranteed to write to a unique bucket. However, if the final table is
              smaller, this guarantee is broken.
                0b0011 1011 0110 0101 - Thread 1
                0b0000 0000 0000 1101 - Thread 2
                                  ^^^
    */
}

pub struct ConcurrentBuilder {
    parallelism: usize,
    table_partitions: usize,
    global_index_tracker: GlobalIndexTracker,
    // Capacity= parallelism * table_partitions
    partitioned_table_senders: Arc<Vec<ClaimOnce<broadcast::Sender<Arc<Vec<HashTable<LocalEntry>>>>>>>,
    partitioned_table_receivers: Arc<Vec<ClaimOnce<broadcast::Receiver<Arc<Vec<HashTable<LocalEntry>>>>>>>,
    // Capacity=1
    overflow_buffer_sender: Arc<ClaimOnce<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>>,
    overflow_buffer_receivers: Arc<Vec<ClaimOnce<broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>>>,
    wait_for_all_overflow_buffers_written: Arc<Barrier>,
    // Capacity=1
    bucket_counts_sender: Arc<ClaimOnce<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>>,
    bucket_counts_receivers: Arc<Vec<ClaimOnce<broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>>>,
    wait_for_all_cells_merged_written: Arc<Barrier>,
    destination_table_sender: Arc<ClaimOnce<(
        // Capacity=1
        broadcast::Sender<usize>,
        // Capacity=1
        broadcast::Sender<Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>>,
    )>>,
    destination_table_size_receivers: Arc<Vec<ClaimOnce<broadcast::Receiver<usize>>>>,
    destination_table_receivers: Arc<Vec<ClaimOnce<broadcast::Receiver<Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>>>>>,
}

impl ConcurrentBuilder {
    pub fn new(
        parallelism: usize,
    ) -> Self {
        let table_partitions = parallelism.next_power_of_two();
        let partitioned_table_sender = broadcast::Sender::new(table_partitions * parallelism);
        let partitioned_table_senders = Arc::new((0..parallelism)
            .map(|_| ClaimOnce::new(partitioned_table_sender.clone()))
            .collect::<Vec<_>>());
        let partitioned_table_receivers = Arc::new((0..parallelism)
            .map(|_| ClaimOnce::new(partitioned_table_sender.subscribe()))
            .collect::<Vec<_>>());
        let overflow_buffer_sender = broadcast::Sender::new(1);
        let overflow_buffer_receivers = Arc::new((0..parallelism)
            .map(|_| ClaimOnce::new(overflow_buffer_sender.subscribe()))
            .collect::<Vec<_>>());
        let bucket_counts_sender = broadcast::Sender::new(1);
        let bucket_counts_receivers = Arc::new((0..parallelism)
            .map(|_| ClaimOnce::new(bucket_counts_sender.subscribe()))
            .collect::<Vec<_>>());
        let destination_table_size_sender = broadcast::Sender::new(1);
        let destination_table_sender = broadcast::Sender::new(1);
        let destination_table_size_receivers = Arc::new((0..parallelism)
            .map(|_| ClaimOnce::new(destination_table_size_sender.subscribe()))
            .collect::<Vec<_>>());
        let destination_table_receivers = Arc::new((0..parallelism)
            .map(|_| ClaimOnce::new(destination_table_sender.subscribe()))
            .collect::<Vec<_>>());

        let wait_for_all_overflow_buffers_written = Arc::new(Barrier::new(parallelism));
        let wait_for_all_cells_merged_written = Arc::new(Barrier::new(parallelism));

        Self {
            parallelism,
            table_partitions,
            global_index_tracker: GlobalIndexTracker::new(),
            partitioned_table_senders,
            partitioned_table_receivers,
            overflow_buffer_sender: Arc::new(ClaimOnce::new(overflow_buffer_sender)),
            overflow_buffer_receivers,
            wait_for_all_overflow_buffers_written,
            bucket_counts_sender: Arc::new(ClaimOnce::new(bucket_counts_sender)),
            bucket_counts_receivers,
            wait_for_all_cells_merged_written,
            destination_table_sender: Arc::new(ClaimOnce::new((
                destination_table_size_sender,
                destination_table_sender,
            ))),
            destination_table_size_receivers,
            destination_table_receivers,
        }
    }

    pub async fn run(
        &self,
        thread_index: usize,
        inputs: Vec<Vec<u64>>
    ) -> ConcurrentBuilderLookup {
        let (lookup, _) = self.run_with_buffer_indices(thread_index, inputs).await;
        lookup
    }

    pub async fn run_with_buffer_indices(
        &self,
        thread_index: usize,
        inputs: Vec<Vec<u64>>
    ) -> (ConcurrentBuilderLookup, Vec<(usize, usize)>) {
        let partitioned_table_sender = self.partitioned_table_senders[thread_index].claim()
            .expect("Partitioned table sender was already claimed");
        let partitioned_table_receiver = self.partitioned_table_receivers[thread_index].claim()
            .expect("Partitioned table receiver was already claimed");
        let overflow_buffer_sender = Arc::clone(&self.overflow_buffer_sender);
        let overflow_buffer_receiver = self.overflow_buffer_receivers[thread_index].claim()
            .expect("Overflow buffer receiver was already claimed");
        let wait_for_all_overflow_buffers_written = Arc::clone(&self.wait_for_all_overflow_buffers_written);
        let bucket_counts_sender = Arc::clone(&self.bucket_counts_sender);
        let bucket_counts_receiver = self.bucket_counts_receivers[thread_index].claim()
            .expect("Bucket counts receiver was already claimed");
        let wait_for_all_cells_merged_written = Arc::clone(&self.wait_for_all_cells_merged_written);
        let destination_table_sender = Arc::clone(&self.destination_table_sender);
        let destination_table_receiver = self.destination_table_receivers[thread_index].claim()
            .expect("Destination table receiver was already claimed");
        let destination_table_size_receiver = self.destination_table_size_receivers[thread_index].claim()
            .expect("Destination table size receiver was already claimed");

        let (partitioned_tables, local_overflows) = accumulate_locally(
            self.table_partitions,
            &self.global_index_tracker,
            inputs,
        ).await;
        let buffer_indices = local_overflows.iter().map(|overflow| (overflow.global_index, overflow.global_offset)).collect::<Vec<_>>();
        // println!("Thread {} finished local accumulation", thread_index);

        let all_tables = share_tables(
            self.parallelism,
            partitioned_tables,
            partitioned_table_sender,
            partitioned_table_receiver,
        ).await;
        // println!("Thread {} finished sharing tables", thread_index);

        let (full_table, full_overflow) = merge_cells_cooperatively(
            self.parallelism,
            thread_index,
            &self.global_index_tracker,
            self.table_partitions,
            all_tables,
            local_overflows,
            overflow_buffer_sender,
            overflow_buffer_receiver,
            wait_for_all_overflow_buffers_written,
            bucket_counts_sender,
            bucket_counts_receiver,
            wait_for_all_cells_merged_written,
            destination_table_sender,
            destination_table_size_receiver,
            destination_table_receiver,
        ).await;

        // Note: we return here once this thread has finished writing to the destination table, but
        // it doesn't mean that the destination table is ready to be read from. The caller should
        // wait for all other threads to complete as well.
        let lookup = ConcurrentBuilderLookup {
            map: full_table,
            overflow: full_overflow,
        };
        (lookup, buffer_indices)
    }
}

pub struct ConcurrentBuilderLookup {
    map: Arc<UnsafeCellSendWrapper<HashTable<(u64, usize)>>>,
    overflow: Arc<UnsafeCellSendWrapper<Vec<usize>>>,
}

impl Lookup for ConcurrentBuilderLookup {
    fn get(&self, hash: u64) -> Vec<usize> {
        let map = unsafe { &*self.map.get() };
        match map.find(hash, |(key, _)| *key == hash) {
            None => vec![],
            Some((_, index)) => {
                let overflow = unsafe { &*self.overflow.get() };
                let mut output = vec![index - 1];
                let mut next = overflow[*index];
                while next != 0 {
                    output.push(next - 1);
                    next = overflow[next];
                }
                output
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use assertables::assert_set_impl_prep;
use std::sync::Arc;
    use std::time::Duration;
    use assertables::assert_set_eq;
    use tokio::task::JoinSet;
    use crate::builder::attempt3::ConcurrentBuilder;
    use crate::lookup::Lookup;

    #[tokio::test(flavor = "multi_thread")]
    pub async fn can_lookup_entries() {
        let builder = Arc::new(ConcurrentBuilder::new(2));
        let inputs1 = vec![vec![1, 2, 3], vec![7, 8, 9]];
        let inputs2 = vec![vec![4, 5, 6], vec![10, 11, 12]];

        let thread1_handle = tokio::task::spawn({
            let builder = builder.clone();
            let inputs = inputs1.clone();
            async move { builder.run_with_buffer_indices(0, inputs).await }
        });
        let thread2_handle = tokio::task::spawn({
            let builder = builder.clone();
            let inputs = inputs2.clone();
            async move { builder.run_with_buffer_indices(1, inputs).await }
        });

        let (lookup1, buffer_indices1) = tokio::time::timeout(Duration::from_secs(3), thread1_handle).await.unwrap().unwrap();
        let (lookup2, buffer_indices2) = tokio::time::timeout(Duration::from_secs(3), thread2_handle).await.unwrap().unwrap();

        for lookup in vec![lookup1, lookup2] {
            for (inputs, buffer_indices) in vec![(&inputs1, &buffer_indices1), (&inputs2, &buffer_indices2)] {
                for (input_index, input) in inputs.iter().enumerate() {
                    for (local_index, hash) in input.iter().enumerate() {
                        let buffer_index = buffer_indices[input_index].0;
                        assert_eq!(
                            lookup.get(*hash),
                            vec![local_index + buffer_index * 3],
                            "Expected value {} to be at index {}, given buffer offset is {}",
                            hash,
                            local_index + buffer_index * 3,
                            buffer_index,
                        );
                    }
                }
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    pub async fn can_lookup_duplicate_entries() {
        let builder = Arc::new(ConcurrentBuilder::new(2));
        let inputs1 = vec![vec![1, 2, 1], vec![1, 8, 9]];
        let inputs2 = vec![vec![4, 1, 6], vec![10, 1, 12]];

        let thread1_handle = tokio::task::spawn({
            let builder = builder.clone();
            let inputs = inputs1.clone();
            async move { builder.run_with_buffer_indices(0, inputs).await }
        });
        let thread2_handle = tokio::task::spawn({
            let builder = builder.clone();
            let inputs = inputs2.clone();
            async move { builder.run_with_buffer_indices(1, inputs).await }
        });

        let (lookup1, buffer_indices1) = tokio::time::timeout(Duration::from_secs(3), thread1_handle).await.unwrap().unwrap();
        let (lookup2, buffer_indices2) = tokio::time::timeout(Duration::from_secs(3), thread2_handle).await.unwrap().unwrap();

        // println!("buffer 1 indices {:?}, buffer 2 indices {:?}, full overflow {:?}", buffer_indices1, buffer_indices2, unsafe { &* overflow1.get() });
        // println!("map index: {:?}", unsafe { &* table1.get() }.find(1, |item| item.0 == 1));

        assert_set_eq!(
            lookup1.get(1),
            vec![
                0 + buffer_indices1[0].1,
                2 + buffer_indices1[0].1,
                0 + buffer_indices1[1].1,
                1 + buffer_indices2[0].1,
                1 + buffer_indices2[1].1,
            ],
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    pub async fn can_lookup_many_entries() {
        let parallelism = 4;
        let batches = 4;
        let batch_size = 10;
        let builder = Arc::new(ConcurrentBuilder::new(parallelism));
        let all_inputs = (0..parallelism).into_iter()
            .map(|thread_index| {
                (0..batches).into_iter()
                    .map(|batch_index| {
                        let min = thread_index * batch_size + batch_index * parallelism * batch_size;
                        (min as u64..min as u64 + batch_size as u64).into_iter().collect::<Vec<u64>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        let mut join_set = JoinSet::new();
        for (thread_index, input) in all_inputs.iter().enumerate() {
            join_set.spawn({
                let builder = builder.clone();
                let input = input.clone();
                async move { (thread_index, builder.run_with_buffer_indices(thread_index, input).await) }
            });
        }
        let mut results = join_set.join_all().await;
        results.sort_by_key(|(thread_index, _)| *thread_index);

        let (lookups, buffer_indices) = results.into_iter()
            .map(|(_, (lookup, buffer_indices))| (lookup, buffer_indices))
            .unzip::<_, _, Vec<_>, Vec<_>>();

        // let x = buffer_indices.iter()
        //     .zip(all_inputs.iter())
        //     .map(|(buffer_indices, input_indices)| {
        //         buffer_indices.iter()
        //             .zip(input_indices.iter())
        //             .map(|((_, buffer_offset), input)| {
        //                 input.iter()
        //                     .enumerate()
        //                     .map(|(local_index, hash)| {
        //                         (*hash, local_index, local_index + buffer_offset, lookup(&outputs[0].0, &outputs[0].1, *hash))
        //                     })
        //                     .collect::<Vec<_>>()
        //             })
        //             .collect::<Vec<_>>()
        //     })
        //     .collect::<Vec<_>>();
        // println!("outputs: {:?}", x);

        for lookup in lookups {
            for (buffer_indices, input_indices) in buffer_indices.iter().zip(all_inputs.iter()) {
                for ((buffer_index, buffer_offset), input) in buffer_indices.iter().zip(input_indices.iter()) {
                    for (local_index, hash) in input.iter().enumerate() {
                        assert_eq!(
                            lookup.get(*hash),
                            vec![local_index + buffer_index * 10],
                        );
                    }
                }
            }
        }
    }
}
