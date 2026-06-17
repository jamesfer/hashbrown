use std::cmp::Ordering;
use std::collections::btree_map::Entry;
use std::collections::BTreeMap;
use std::vec::Vec;
use crate::builder::global_index_tracker::GlobalIndexTracker;

#[derive(Debug, Clone)]
pub struct OrderedBuffer {
    pub global_buffer_index: usize,
    pub buffer_data: Vec<usize>,
}

impl PartialEq for OrderedBuffer {
    fn eq(&self, other: &Self) -> bool {
        self.global_buffer_index == other.global_buffer_index
            && self.buffer_data == other.buffer_data
    }
}

#[derive(Debug, Clone)]
pub struct ChainItem {
    pub thread_index: usize,
    pub buffer_index: usize,
    pub vec_index: usize,
}

impl PartialEq for ChainItem {
    fn eq(&self, other: &Self) -> bool {
        self.thread_index == other.thread_index
            && self.buffer_index == other.buffer_index
            && self.vec_index == other.vec_index
    }
}

#[derive(Debug, Clone)]
pub struct HistogramEntry {
    pub hash: u64,
    pub value: usize,
    pub end_of_chain: ChainItem,
}

impl PartialEq for HistogramEntry {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
            && self.value == other.value
            && self.end_of_chain == other.end_of_chain
    }
}

async fn locally_accumulate_input_into_histogram(
    thread_index: usize,
    global_index_tracker: &GlobalIndexTracker,
    input: Vec<u64>,
    buckets: &mut Vec<BTreeMap<u64, HistogramEntry>>,
) -> OrderedBuffer {
    let bucket_mask = buckets.len() - 1;
    let buffer_offset = global_index_tracker.allocate(input.len()).await;
    let mut overflow = vec![0usize; input.len()];
    for (local_index, hash) in input.into_iter().enumerate() {
        let insertion_index = (buffer_offset.size + local_index) + 1;

        let bucket_index = hash as usize & (bucket_mask);
        let mut bucket = &mut buckets[bucket_index];
        match bucket.entry(hash) {
            Entry::Vacant(entry) => {
                entry.insert(HistogramEntry {
                    hash,
                    value: insertion_index,
                    end_of_chain: ChainItem {
                        thread_index,
                        buffer_index: buffer_offset.index,
                        vec_index: local_index,
                    },
                });
            },
            Entry::Occupied(mut entry) => {
                let existing_value = std::mem::replace(&mut entry.get_mut().value, insertion_index);
                overflow[local_index] = existing_value;
            },
        };
    }

    OrderedBuffer {
        global_buffer_index: buffer_offset.index,
        buffer_data: overflow,
    }
}

pub async fn locally_accumulate_histograms(
    thread_index: usize,
    histogram_bucket_count: usize,
    global_index_tracker: &GlobalIndexTracker,
    inputs: Vec<Vec<u64>>,
) -> (Vec<BTreeMap<u64, HistogramEntry>>, Vec<OrderedBuffer>){
    assert!(histogram_bucket_count.is_power_of_two());

    let mut buckets = vec![BTreeMap::new(); histogram_bucket_count];
    let mut overflow_buffers = Vec::new();

    for input in inputs {
        overflow_buffers.push(locally_accumulate_input_into_histogram(
            thread_index,
            global_index_tracker,
            input,
            &mut buckets,
        ).await);
    }

    (buckets, overflow_buffers)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use assertables::assert_iter_eq;
    use crate::builder::global_index_tracker::GlobalIndexTracker;
    use crate::builder::histogram_local_accumulate::{locally_accumulate_histograms, ChainItem, HistogramEntry, OrderedBuffer};

    // TODO Doesn't create empty buckets

    #[tokio::test]
    pub async fn single_threaded_without_duplicates() {
        let global_index_tracker = GlobalIndexTracker::new();
        let data = vec![vec![1, 2, 3], vec![4, 5, 6]];
        let (buckets, overflow) = locally_accumulate_histograms(0, 16, &global_index_tracker, data).await;

        // There should be no overflows
        assert_iter_eq!(overflow, vec![
            OrderedBuffer {
                global_buffer_index: 0,
                buffer_data: vec![0, 0, 0],
            },
            OrderedBuffer {
                global_buffer_index: 1,
                buffer_data: vec![0, 0, 0],
            },
        ]);

        for i in 1usize..=6 {
            let mut expected = BTreeMap::new();
            expected.insert(i as u64, HistogramEntry {
                hash: i as u64,
                value: i,
                end_of_chain: ChainItem {
                    thread_index: 0,
                    buffer_index: if i <= 3 { 0 } else { 1 },
                    // The vec index of the first buffer is the value - 1, the second buffer is the
                    // value - 1 - len(first vec)
                    vec_index: i - if i <= 3 { 1 } else { 4 },
                },
            });
            assert_eq!(buckets[i], expected);
        }
    }

    #[tokio::test]
    pub async fn single_threaded_with_duplicates() {
        let global_index_tracker = GlobalIndexTracker::new();
        let data = vec![vec![1, 2, 3], vec![1, 2, 1]];
        let (buckets, overflow) = locally_accumulate_histograms(0, 16, &global_index_tracker, data).await;

        assert_iter_eq!(overflow, vec![
            // First buffer has no overflows
            OrderedBuffer {
                global_buffer_index: 0,
                buffer_data: vec![0, 0, 0],
            },
            // Second buffer contains the duplicate indices
            OrderedBuffer {
                global_buffer_index: 1,
                buffer_data: vec![1, 2, 4],
            },
        ]);

        let mut expected = BTreeMap::new();
        expected.insert(1, HistogramEntry {
            hash: 1,
            value: 6, // Index of the last 1
            end_of_chain: ChainItem { // Details from the first 1
                thread_index: 0,
                buffer_index: 0,
                vec_index: 0,
            },
        });
        assert_eq!(buckets[1], expected);

        let mut expected = BTreeMap::new();
        expected.insert(2, HistogramEntry {
            hash: 2,
            value: 5, // Index of the last 2
            end_of_chain: ChainItem { // Details from the first 1
                thread_index: 0,
                buffer_index: 0,
                vec_index: 1,
            },
        });
        assert_eq!(buckets[2], expected);
    }
}
