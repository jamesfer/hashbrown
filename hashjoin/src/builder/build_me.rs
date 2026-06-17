// use std::boxed::Box;
// use std::cell::UnsafeCell;
// use std::collections::BTreeMap;
// use std::future::Future;
// use std::ops::Deref;
// use std::sync::{Arc};
// use std::vec::Vec;
// use arrow::array::{Array, PrimitiveArray};
// use tokio::sync::broadcast;
// use tokio::sync::broadcast::error::RecvError;
// use hashbrown::{HashMap, HashTable};
// use crate::builder::histogram_local_accumulate::{HistogramEntry, OrderedBuffer};
// use crate::builder::utils::{ClaimOnce, UnsafeCellSendWrapper};
//
//
// // Step 1: accumulate all the items into N collections
// // Step 2: merge collections to produce the number of items per bucket
// // Step 3: write those items o the final map in parallel
//
// // pub fn build_hash_map_from_histogram<T>(histograms: Vec<Vec<HistogramItem<T>>>) {
// //     let table = Arc::new(UnsafeCell::new(HashMap::<u64, u64>::new()));
// //
// //     for histogram in histograms {
// //         let table = table.clone();
// //         std::thread::spawn(move || {
// //             for item in histogram {
// //                 let mut table = unsafe { &mut *table.get() };
// //                 let count = table.entry(item.hash).or_insert(0);
// //                 *count += 1;
// //             }
// //         });
// //     }
// // }
//
// // // Determines the first bucket an item should be allocated to
// // fn bucket_for(hash: u64, buckets: usize) -> usize {
// //     assert!(buckets.is_power_of_two());
// //     // Check that we are running on a 64-bit system
// //     assert_eq!(std::mem::<usize>::size_of(), std::mem::<u64>::size_of());
// //
// //     // This is the same as hash % buckets since buckets is a power of two
// //     (hash as usize) & (buckets - 1)
// // }
// //
// //
// // fn accumulate_locally<T>(input: Vec<(u64, T)>) -> Vec<BTreeMap<u64, T>> {
// //
// // }
//
// // Done
// fn merge_two_sorted_iterators<T, KF, K, F>(
//     left: impl Iterator<Item = T>,
//     right: impl Iterator<Item = T>,
//     mut key_fn: KF,
//     mut on_duplicate: F,
// ) -> impl Iterator<Item = Result<T, T>>
// where
//     KF: FnMut(&T) -> K,
//     K: Ord,
//     F: FnMut(&mut T, T)
// {
//     let mut left = left.peekable();
//     let mut right = right.peekable();
//
//     let mut left_item = left.next();
//     let mut right_item = right.next();
//
//     std::iter::from_fn(move || {
//         // This loop is only triggered if the left and right values are equal. In all other cases,
//         // the match statement returns explicitly
//         loop {
//             return match (left_item.take(), right_item.take()) {
//                 (Some(mut left_value), Some(right_value)) => {
//                     let left_key = key_fn(&left_value);
//                     let right_key = key_fn(&right_value);
//
//                     if left_key == right_key {
//                         on_duplicate(&mut left_value, right_value);
//                         left_item = Some(left_value);
//                         right_item = right.next();
//                         // Loop again to compare the same left item against the next right item
//                         continue;
//                     }
//
//                     if left_key < right_key {
//                         right_item = Some(right_value);
//                         left_item = left.next();
//                         Some(Ok(left_value))
//                     } else {
//                         left_item = Some(left_value);
//                         right_item = right.next();
//                         Some(Err(right_value))
//                     }
//                 }
//                 (Some(left_value), None) => {
//                     left_item = left.next();
//                     Some(Ok(left_value))
//                 },
//                 (None, Some(right_value)) => {
//                     right_item = right.next();
//                     Some(Err(right_value))
//                 },
//                 (None, None) => {
//                     None
//                 },
//             }
//         }
//     })
// }
//
// // Done
// fn merge_all_iterators<T, KF, K, F, I>(
//     iterators: impl IntoIterator<Item = I>,
//     mut key_fn: KF,
//     mut on_duplicate: F,
// ) -> Box<dyn Iterator<Item = T>>
// where
//     KF: FnMut(&T) -> K,
//     K: Ord,
//     F: FnMut(&mut (usize, T), (usize, T)),
//     I: IntoIterator<Item = T> + 'static,
//     T: 'static,
// {
//     iterators.into_iter()
//         .enumerate()
//         // Iterators must be boxed here so that reduce can accept the right types
//         .map(|(index, iterator)| -> Box<dyn Iterator<Item = (usize, T)>> { Box::new(iterator.into_iter().map(move |item| (index, item))) })
//         .reduce(|left, right| {
//             Box::new(
//                 merge_two_sorted_iterators(
//                     left,
//                     right,
//                     |(_, item)| key_fn(item),
//                     |left, right| on_duplicate(left, right)
//                 )
//                     .map(|r| r.unwrap_or_else(|v| v))
//                     .collect::<Vec<_>>()
//                     .into_iter()
//             )
//         })
//         .map(|iter| Box::new(iter.map(|(_, item)| item)) as Box<dyn Iterator<Item = T>>)
//         .unwrap_or(Box::new(std::iter::empty()))
// }
//
// fn merge_all_histograms_2(histogram_buckets_to_combine: Vec<BTreeMap<u64, HistogramEntry>>, all_overflows: &mut Vec<&mut Vec<OrderedBuffer>>) -> Vec<HistogramEntry> {
//     let on_equal_handler = |(left_index, left_item): &mut (usize, HistogramEntry), (right_index, right_item): (usize, HistogramEntry)| {
//         // These two items have the same key, so their chains need to be linked. The right item
//         // (the head of the right chain will be moved to the end of the left chain). Then the left
//         // item will be updated to store the end of the right chain.
//
//         let end_of_left = &left_item.end_of_chain;
//         let mut end_of_left_overflow_buffer = &mut all_overflows[end_of_left.thread_index][end_of_left.buffer_index];
//         end_of_left_overflow_buffer.buffer_data[end_of_left.vec_index] = right_item.value;
//         left_item.end_of_chain = right_item.end_of_chain;
//     };
//
//     let buckets_to_merge = histogram_buckets_to_combine.into_iter()
//         .map(|buckets| buckets.into_values());
//     merge_all_iterators(
//         buckets_to_merge,
//         |item| item.hash,
//         on_equal_handler,
//     )
//         .collect::<Vec<_>>()
// }
//
// // source_items is a list of the merged items from a single bucket of each local map.
// fn increment_bucket_quantities(
//     source_items: &Vec<HistogramEntry>,
//     destination_bucket_counts: &mut Vec<usize>,
// ) {
//     assert!(destination_bucket_counts.len().is_power_of_two());
//
//     let bucket_mask = destination_bucket_counts.len() - 1;
//     for item in source_items {
//         let bucket = item.hash as usize & (bucket_mask);
//         destination_bucket_counts[bucket] += 1;
//     }
// }
//
// // After merging all the buckets of each individual histogram, we need to calculate the total number
// // of items that will be in each bucket of the final map. This will indicate which items fall in
// // which bucket.
// fn calculate_total_bucket_quantities(
//     all_source_items: &Vec<Vec<HistogramEntry>>,
//     destination_bucket_count: usize,
// ) -> Vec<usize> {
//     assert!(destination_bucket_count.is_power_of_two());
//
//     let mut bucket_quantities = vec![0; destination_bucket_count];
//     for source_items in all_source_items {
//         increment_bucket_quantities(source_items, &mut bucket_quantities);
//     }
//     bucket_quantities
// }
//
// pub struct LocalHistogram {
//     pub buckets: Vec<ClaimOnce<BTreeMap<u64, HistogramEntry>>>,
//     pub overflows: Arc<UnsafeCellSendWrapper<Vec<OrderedBuffer>>>,
// }
//
// fn merge_local_histogram_buckets(
//     thread_index: usize,
//     total_threads: usize,
//     all_local_histograms: Vec<&LocalHistogram>,
// ) -> Vec<Vec<HistogramEntry>> {
//     // Extract a mutable reference from all the overflow buffers. Each thread running this function
//     // will have a mutable reference to the overflow buffer, however, they will each mutate
//     // different parts of the buffer.
//     let mut all_overflows = all_local_histograms.iter()
//         .map(|local_histogram| unsafe { &mut *local_histogram.overflows.get() })
//         .collect::<Vec<_>>();
//
//     // For now, all histograms need to have the same number of buckets
//     assert!(all_local_histograms.len() > 0);
//     let bucket_count = all_local_histograms[0].buckets.len();
//     assert!(all_local_histograms.iter().all(|local_histogram| local_histogram.buckets.len() == bucket_count));
//
//     // Take the buckets that this thread owns. If any of the locks are occupied or the bucket is
//     // empty, we panic as that is a logical error.
//     let ceil_histograms_per_thread = bucket_count.div_ceil(total_threads);
//     let remainder = all_local_histograms.len() % total_threads;
//     let start_index = thread_index * ceil_histograms_per_thread - thread_index.saturating_sub(remainder);
//     let end_index = start_index + ceil_histograms_per_thread - (if thread_index < remainder { 1 } else { 0 });
//
//     println!("Owned bucket range: {}..{}, ceil_histograms_per_thread {}, remainder {}", start_index, end_index, ceil_histograms_per_thread, remainder);
//     let x = (start_index..end_index).into_iter()
//         .map(|bucket_index| {
//             all_local_histograms.iter()
//                 .enumerate()
//                 .map(|(index, local_histogram)|
//                     local_histogram.buckets[bucket_index].claim()
//                         .map_err(|err| format!("Thread {} failed to claim bucket {} from histogram {}: {:?}", thread_index, bucket_index, index, err))
//                 )
//                 .collect::<Result<Vec<_>, _>>()
//                 .unwrap()
//         })
//         .collect::<Vec<_>>();
//     println!("x: {:?}", x);
//
//     // Merge the buckets together, excluding duplicates, to produce an iterator of the values that
//     // are in each source bucket
//     let y = x.into_iter()
//         .map(|buckets| {
//             merge_all_histograms_2(buckets, &mut all_overflows)
//         })
//         .collect::<Vec<_>>();
//
//     y
// }
//
// fn split_buckets(destination_bucket_count: usize, source_buckets: Vec<Vec<HistogramEntry>>) -> HashMap<usize, Vec<HistogramEntry>> {
//     let mut destination_buckets = HashMap::new();
//     for source_bucket in source_buckets {
//         for item in source_bucket {
//             let destination_bucket_index = item.hash as usize & (destination_bucket_count - 1);
//             destination_buckets.entry(destination_bucket_index)
//                 .or_insert_with(Vec::new)
//                 .push(item);
//         }
//     }
//     destination_buckets
// }
//
// fn merge_bucket_counts_vec(
//     max_bucket_size: usize,
//     left: &Vec<(usize, usize)>,
//     right: &Vec<(usize, usize)>,
// ) -> (Vec<(usize, usize)>, Vec<(usize, usize)>) {
//     let mut left_index = 0;
//     let mut right_index = 0;
//
//     // Increment indexes until we find the split point
//     let mut included_output = Vec::with_capacity(left_index + right_index);
//     let mut carry_over_output = Vec::with_capacity(left_index + right_index);
//     let mut total = 0;
//     while left_index < left.len() && right_index < right.len() {
//         let (left_level, left_count) = left[left_index];
//         let (right_level, right_count) = right[right_index];
//         assert_ne!(left_level, right_level, "Duplicate levels found in the bucket counts");
//
//         if left_level < right_level {
//             if total < max_bucket_size {
//                 total += left_count;
//                 let overflow = total.saturating_sub(max_bucket_size); // max(0, total - max_bucket_size);
//                 included_output.push((left_level, left_count - overflow));
//
//                 if overflow > 0 {
//                     carry_over_output.push((left_level, overflow));
//                 }
//             } else {
//                 carry_over_output.push((left_level, left_count));
//             }
//             left_index += 1;
//         } else {
//             if total < max_bucket_size {
//                 total += right_count;
//                 let overflow = total.saturating_sub(max_bucket_size); // max(0, total - max_bucket_size);
//                 included_output.push((right_level, right_count - overflow));
//
//                 if overflow > 0 {
//                     carry_over_output.push((right_level, overflow));
//                 }
//             } else {
//                 carry_over_output.push((right_level, left_count));
//             }
//             right_index += 1;
//         }
//     }
//
//     // Finish consuming both iterators. Order doesn't matter here as one of them is guaranteed to
//     // be finished
//     for i in left_index..left.len() {
//         let (left_level, left_count) = left[i];
//         if total < max_bucket_size {
//             total += left_count;
//             let overflow = total.saturating_sub(max_bucket_size); // max(0, total - max_bucket_size);
//             included_output.push((left_level, left_count - overflow));
//
//             if overflow > 0 {
//                 carry_over_output.push((left_level, overflow));
//             }
//         } else {
//             carry_over_output.push((left_level, left_count));
//         }
//     }
//
//     for i in right_index..right.len() {
//         let (right_level, right_count) = right[i];
//         if total < max_bucket_size {
//             total += right_count;
//             let overflow = total.saturating_sub(max_bucket_size); // max(0, total - max_bucket_size);
//             included_output.push((right_level, right_count - overflow));
//
//             if overflow > 0 {
//                 carry_over_output.push((right_level, overflow));
//             }
//         } else {
//             carry_over_output.push((right_level, right_count));
//         }
//     }
//
//     (included_output, carry_over_output)
// }
//
// #[cfg(test)]
// mod merge_tests {
//     use crate::builder::build_me::merge_bucket_counts_vec;
//
//     #[test]
//     fn consumes_all_of_left() {
//         assert_eq!(
//             merge_bucket_counts_vec(8, &vec![(0, 3), (1, 2)], &vec![]),
//             (vec![(0, 3), (1, 2)], vec![]),
//         );
//     }
//
//     #[test]
//     fn consumes_all_of_right() {
//         assert_eq!(
//             merge_bucket_counts_vec(8, &vec![], &vec![(0, 3), (1, 2)]),
//             (vec![(0, 3), (1, 2)], vec![]),
//         );
//     }
//
//     #[test]
//     fn consumes_both() {
//         assert_eq!(
//             merge_bucket_counts_vec(8, &vec![(0, 3), (2, 1)], &vec![(1, 2), (3, 1)]),
//             (vec![(0, 3), (1, 2), (2, 1), (3, 1)], vec![]),
//         );
//     }
//
//     #[test]
//     fn can_completely_fill_a_bucket() {
//         assert_eq!(
//             merge_bucket_counts_vec(8, &vec![(0, 3), (2, 1)], &vec![(1, 2), (3, 2)]),
//             (vec![(0, 3), (1, 2), (2, 1), (3, 2)], vec![]),
//         );
//     }
//
//     #[test]
//     fn will_overflow_all_results() {
//         assert_eq!(
//             merge_bucket_counts_vec(8, &vec![(0, 3), (2, 1), (5, 5)], &vec![(1, 2), (3, 2), (6, 2)]),
//             (vec![(0, 3), (1, 2), (2, 1), (3, 2)], vec![(5, 5), (6, 2)]),
//         );
//     }
//
//     #[test]
//     fn will_split_a_left_bucket() {
//         assert_eq!(
//             merge_bucket_counts_vec(8, &vec![(0, 3), (2, 8)], &vec![(1, 2), (3, 2), (6, 2)]),
//             (vec![(0, 3), (1, 2), (2, 3)], vec![(2, 5), (3, 2), (6, 2)]),
//         );
//     }
//
//     #[test]
//     fn will_split_a_right_bucket() {
//         assert_eq!(
//             merge_bucket_counts_vec(8, &vec![(0, 3), (2, 1), (5, 5)], &vec![(1, 2), (3, 8), (6, 2)]),
//             (vec![(0, 3), (1, 2), (2, 1), (3, 2)], vec![(3, 6), (5, 5), (6, 2)]),
//         );
//     }
//
//     #[test]
//     fn will_split_a_left_bucket_after_right_finished() {
//         assert_eq!(
//             merge_bucket_counts_vec(8, &vec![(0, 3), (2, 1), (3, 8)], &vec![(1, 2)]),
//             (vec![(0, 3), (1, 2), (2, 1), (3, 2)], vec![(3, 6)]),
//         );
//     }
//
//     #[test]
//     fn will_split_a_right_bucket_after_left_finished() {
//         assert_eq!(
//             merge_bucket_counts_vec(8, &vec![(1, 2)], &vec![(0, 3), (2, 1), (3, 8)]),
//             (vec![(0, 3), (1, 2), (2, 1), (3, 2)], vec![(3, 6)]),
//         );
//     }
// }
//
// fn spread_out_items(max_bucket_size: usize, buckets: &Vec<usize>) -> Vec<Vec<(usize, usize)>> {
//     assert!(buckets.len().is_power_of_two());
//
//     let mut destination_buckets = vec![Vec::<(usize, usize)>::new(); buckets.len()];
//     for (index, count) in buckets.iter().enumerate().filter(|(_, count)| **count > 0) {
//         let mut level = 0;
//         let mut stride = 0;
//         let mut write_index = index;
//         let mut remaining = *count;
//         while remaining > 0 && stride <= buckets.len() - 1 {
//             let mut destination = &mut destination_buckets[write_index];
//             let existing_count = destination.iter().map(|(_, count)| *count).sum::<usize>();
//             if existing_count < max_bucket_size {
//                 let space = max_bucket_size - existing_count;
//                 destination.push((level, remaining.min(space)));
//                 remaining = remaining.saturating_sub(space);
//             }
//
//             level += 1;
//             stride += 1;
//             write_index = (write_index + stride) & (buckets.len() - 1);
//         }
//
//         assert_eq!(remaining, 0);
//     }
//
//     destination_buckets
//
//
//
//     // let mut current_index = 0;
//     // // let mut loops = 0;
//     // let mut empty_carries = 0;
//     // while empty_carries < buckets.len() {
//     //     // if loops > 2 * buckets.len() {
//     //     //     panic!("Infinite loop detected");
//     //     // }
//     //     // loops += 1;
//     //
//     //     let mut carry = &mut carry_over[current_index];
//     //     if carry.is_empty() {
//     //         empty_carries += 1;
//     //         current_index = (current_index + 1) & (buckets.len() - 1);
//     //         continue;
//     //     } else {
//     //         empty_carries = 0;
//     //     }
//     //
//     //     let to_be_inserted = std::mem::replace(carry, vec![]);
//     //     let (included, remainders) = merge_bucket_counts_vec(max_bucket_size, &to_be_inserted, &destination_buckets[current_index]);
//     //     destination_buckets[current_index] = included;
//     //
//     //     // Push the carry over values down the line
//     //     for (level, count) in remainders {
//     //         // Perform a probing to find the next bucket for these values
//     //         let next_level = level + 1;
//     //         // The stride normally increases by 8 (depending on group size). Since we consider a
//     //         // group a single index, we increment by 1 each time, so the stride at level n is n
//     //         let stride = next_level;
//     //         let next_index = (current_index + stride) & (buckets.len() - 1);
//     //
//     //         let new_carry = (next_level, count);
//     //         let existing_remainders = &mut carry_over[next_index];
//     //         let insertion_index = match existing_remainders.binary_search_by_key(&next_level, |(level, _)| *level) {
//     //             Ok(_) => {
//     //                 panic!("Duplicate level found in carry over");
//     //             }
//     //             Err(insertion_index) => insertion_index,
//     //         };
//     //         existing_remainders.insert(insertion_index, new_carry);
//     //     }
//     //
//     //     current_index = (current_index + 1) & (buckets.len() - 1);
//     // }
//     //
//     // destination_buckets
// }
//
// #[cfg(test)]
// mod spread_items_tests {
//     #[test]
//     fn can_spread_out_a_single_bucket() {
//         assert_eq!(
//             super::spread_out_items(8, &vec![10, 0]),
//             vec![vec![(0, 8)], vec![(1, 2)]],
//         );
//     }
//
//     #[test]
//     fn can_spread_out_a_single_bucket_circularly() {
//         assert_eq!(
//             super::spread_out_items(8, &vec![0, 12]),
//             vec![vec![(1, 4)], vec![(0, 8)]],
//         );
//     }
//
//     #[test]
//     fn can_spread_out_a_single_bucket_into_many() {
//         assert_eq!(
//             super::spread_out_items(8, &vec![0, 26, 0, 0]),
//             vec![vec![(2, 8)], vec![(0, 8)], vec![(1, 8)], vec![(3, 2)]],
//         );
//     }
//
//     // #[test]
//     // fn higher_levels_should_take_priority() {
//     //     assert_eq!(
//     //         super::spread_out_items(8, &vec![1, 26, 0, 0]),
//     //         vec![vec![(2, 8)], vec![(0, 7), (1, 1)], vec![(1, 8)], vec![(3, 3)]],
//     //     );
//     // }
// }
//
//
// fn write_values_to_hash_map(max_bucket_size: usize, hash_map: &mut HashTable<usize>, all_counts: &Vec<usize>, all_owned_values: HashMap<usize, Vec<HistogramEntry>>) {
//     let mut destination_counts = vec![0; all_counts.len()];
//
//     // Loop over all the buckets and pretend to write the values in each of the spread out buckets.
//     // If the bucket is one we own, we can actually write the values to the real hashmap
//     for bucket_index in 0..all_counts.len() {
//         let source_count = all_counts[bucket_index];
//
//         let values = all_owned_values.get(&bucket_index);
//         let mut values_index = 0;
//
//         let mut level = 0;
//         let mut stride = 0;
//         let mut write_bucket_index = bucket_index;
//         let mut remaining = source_count;
//         while remaining > 0 && stride <= all_counts.len() - 1 {
//             let mut existing_count = &mut destination_counts[write_bucket_index];
//             if *existing_count < max_bucket_size {
//                 // Determine how many values we can write to this bucket
//                 let space = max_bucket_size - *existing_count;
//                 let values_written = remaining.min(space);
//
//                 // If we own this bucket, actually write that many values to the real hashmap
//                 if let Some(values) = values {
//                     assert!(values_index + values_written >= values.len(), "Attempted to write more values than were available");
//                     for _ in 0..values_written {
//                         let value_to_write = &values[values_index];
//                         let pos = write_bucket_index * max_bucket_size + *existing_count;
//                         hash_map.insert_directly(value_to_write.hash, value_to_write.value, pos)
//                             .unwrap();
//
//                         *existing_count += 1;
//                         values_index += 1;
//                         remaining -= 1;
//                     }
//                 } else {
//                     // Update the counts
//                     *existing_count += values_written;
//                     remaining -= values_written;
//                 }
//             }
//
//             // Perform a probe_seq
//             level += 1;
//             stride += 1;
//             write_bucket_index = (write_bucket_index + stride) & (all_counts.len() - 1);
//         }
//
//         assert_eq!(remaining, 0);
//     }
// }
//
// pub async fn write_local_histograms_to_shared_map(
//     thread_index: usize,
//     total_threads: usize,
//     destination_bucket_count: usize,
//     completed_local_histogram: LocalHistogram,
//     completed_local_histogram_sender: broadcast::Sender<Arc<LocalHistogram>>,
//     mut completed_local_histogram_receiver: broadcast::Receiver<Arc<LocalHistogram>>,
//     local_bucket_size_sender: broadcast::Sender<usize>,
//     mut local_bucket_size_receiver: broadcast::Receiver<usize>,
//     destination_bucket_size_vector_sender: Option<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>,
//     mut destination_bucket_size_vector_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>,
//     destination_bucket_sizes_written_sender: broadcast::Sender<()>,
//     mut destination_bucket_sizes_written_receiver: broadcast::Receiver<()>,
//     destination_map_sender: Option<broadcast::Sender<Arc<UnsafeCellSendWrapper<HashTable<usize>>>>>,
//     mut destination_map_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<HashTable<usize>>>>,
//     destination_map_written_sender: broadcast::Sender<()>,
//     mut destination_map_written_receiver: broadcast::Receiver<()>,
// ) -> Arc<UnsafeCellSendWrapper<HashTable<usize>>> {
//     // Share the local histogram to all threads
//     completed_local_histogram_sender.send(Arc::new(completed_local_histogram))
//         // Error contains no interesting details, so we can drop it
//         .map_err(|err| ())
//         .expect("Failed to send local histogram");
//
//     // Drop the sender to ensure that the receiver eventually completes
//     drop(completed_local_histogram_sender);
//
//     // Wait for all threads to complete their local histograms
//     let mut all_local_histograms = Vec::with_capacity(total_threads);
//     loop {
//         match completed_local_histogram_receiver.recv().await {
//             Ok(histogram) => all_local_histograms.push(histogram),
//             Err(RecvError::Closed) => {
//                 assert_eq!(all_local_histograms.len(), total_threads, "Not all local histograms were received");
//                 break;
//             },
//             Err(RecvError::Lagged(_)) => {
//                 // This should never happen
//                 panic!("Local histogram receiver lagged");
//             },
//         };
//     }
//
//
//     // Merge the buckets owned by this thread together
//     // TODO could be an iterator in future
//     let unique_owned_buckets = merge_local_histogram_buckets(
//         thread_index,
//         total_threads,
//         all_local_histograms.iter().map(|histogram| histogram.as_ref()).collect(),
//     );
//
//     // Publish all the sizes of the buckets so we can count how many total items there are
//     println!("Unique owned buckets: {:?}", unique_owned_buckets);
//     for owned_bucket in unique_owned_buckets.iter() {
//         local_bucket_size_sender.send(owned_bucket.len())
//             .map_err(|err| ())
//             .expect("Failed to send local bucket size");
//     }
//     drop(local_bucket_size_sender);
//
//
//     // Split the owned buckets into their destination buckets and count the number of entries in
//     // each
//     // TODO replace the hash map with a vector paired with an index vector
//     let destination_buckets = split_buckets(destination_bucket_count, unique_owned_buckets);
//
//
//     // Now calculate the total number of all items across all threads
//     let mut total_size = 0;
//     let mut sizes_sent = 0;
//     loop {
//         match local_bucket_size_receiver.recv().await {
//             Ok(size) => {
//                 total_size += size;
//                 sizes_sent += 1;
//             },
//             Err(RecvError::Closed) => {
//                 println!("Bucket size receiver finished after receiving {} sizes", sizes_sent);
//                 break;
//             },
//             Err(RecvError::Lagged(_)) => {
//                 // This should never happen
//                 panic!("Local bucket size receiver lagged");
//             },
//         };
//     }
//
//
//     // Allocate the size vector if we are the thread responsible for it
//     let destination_bucket_sizes_result =
//         if let Some(destination_bucket_size_vector_sender) = destination_bucket_size_vector_sender {
//             // TODO calculate based on the total size
//             let destination_bucket_sizes = Arc::new(UnsafeCellSendWrapper::new(UnsafeCell::new(vec![0; destination_bucket_count])));
//             destination_bucket_size_vector_sender.send(destination_bucket_sizes.clone())
//                 .map_err(|err| ())
//                 .expect("Failed to send destination bucket sizes");
//             drop(destination_bucket_size_vector_sender);
//             drop(destination_bucket_size_vector_receiver);
//             Ok(destination_bucket_sizes)
//         } else {
//             Err(async move {
//                 let destination_bucket_sizes = destination_bucket_size_vector_receiver.recv().await
//                     .expect("Failed to receive destination bucket sizes");
//                 drop(destination_bucket_size_vector_receiver);
//                 destination_bucket_sizes
//             })
//         };
//
//     // Allocate the final hash map if we are the thread responsible for it
//     let destination_map_result =
//         if let Some(destination_map_sender) = destination_map_sender {
//             let table = HashTable::with_fixed_items(total_size);
//             assert_eq!(table.bucket_count(), destination_bucket_count, "Provided destination_bucket_count doesn't match the actual number of buckets allocated by the HashMap");
//             assert!(table.bucket_count() > 8, "We don't handle the case where the number of buckets is less than the group size very well");
//             println!("Created table, size {}, total_size {}", table.len(), total_size);
//
//             let destination_map = Arc::new(UnsafeCellSendWrapper::new(UnsafeCell::new(table)));
//             destination_map_sender.send(destination_map.clone())
//                 .map_err(|err| ())
//                 .expect("Failed to send destination map");
//             drop(destination_map_sender);
//             drop(destination_map_receiver);
//             Ok(destination_map)
//         } else {
//             Err(async move {
//                 let destination_map = destination_map_receiver.recv().await
//                     .expect("Failed to receive destination map");
//                 drop(destination_map_receiver);
//                 destination_map
//             })
//         };
//
//
//     // Write the size of our owned buckets to the destination size vector
//     // Safety: no other thread is writing to the same vector of this thread
//     let destination_bucket_sizes = match destination_bucket_sizes_result {
//         Ok(vec) => vec,
//         Err(fut) => fut.await,
//     };
//     let destination_bucket_sizes = unsafe { &mut *destination_bucket_sizes.get() };
//     for (index, items) in destination_buckets.iter() {
//         destination_bucket_sizes[*index] = items.len();
//     }
//
//     // Indicate that we have finished publishing our bucket sizes
//     destination_bucket_sizes_written_sender.send(())
//         .map_err(|err| ())
//         .expect("Failed to send destination bucket sizes written");
//     drop(destination_bucket_sizes_written_sender);
//
//     // Wait for all threads to do the same
//     let mut received = 0;
//     loop {
//         match destination_bucket_sizes_written_receiver.recv().await {
//             Ok(()) => {
//                 received += 1;
//             },
//             Err(RecvError::Closed) => {
//                 assert_eq!(received, total_threads, "Not all bucket sizes were received");
//                 break;
//             },
//             Err(RecvError::Lagged(_)) => {
//                 // This should never happen
//                 panic!("Bucket sizes receiver lagged");
//             },
//         };
//     }
//     drop(destination_bucket_sizes_written_receiver);
//
//     // Wait for the destination map to arrive
//     let destination_map = match destination_map_result {
//         Ok(map) => map,
//         Err(fut) => fut.await,
//     };
//     let destination_map_ref = unsafe { &mut *destination_map.get() };
//
//     // Start writing the buckets this thread owns to the destination map and then indicate that we
//     // are finished
//     let max_bucket_size = 8;
//     write_values_to_hash_map(max_bucket_size, destination_map_ref, destination_bucket_sizes, destination_buckets);
//     destination_map_written_sender.send(())
//         .map_err(|err| ())
//         .expect("Failed to send destination map written");
//     drop(destination_map_written_sender);
//
//     // Wait for all threads to do the same
//     let mut received = 0;
//     loop {
//         match destination_map_written_receiver.recv().await {
//             Ok(()) => {
//                 received += 1;
//             },
//             Err(RecvError::Closed) => {
//                 assert_eq!(received, total_threads, "Not all bucket sizes were received");
//                 break;
//             },
//             Err(RecvError::Lagged(_)) => {
//                 // This should never happen
//                 panic!("Bucket sizes receiver lagged");
//             },
//         };
//     }
//     drop(destination_map_written_receiver);
//
//     destination_map
// }
//
// fn arr() {
//     let x: PrimitiveArray<u64> = 1;
//     let mut v: Vec<u64>;
//     let mut b: BTreeMap<u64, usize>;
//
//     v.sort_unstable();
//     v.insert()
// }
//
//
//
// #[cfg(test)]
// mod tests {
//     use crate::control::Group;
//     use crate::raw::RawTable;
//
//     #[test]
//     pub fn can_directly_write_to_an_index_in_an_empty_map() {
//         let mut map = RawTable::<u64>::with_capacity(1);
//         let bucket_mask = map.get_bucket_mask();
//
//         let item = 1u64;
//         let hash = 1234u64;
//
//         // Calculate the initial position this item would normally be stored
//         let pos = (hash as usize) & bucket_mask;
//
//         // Insert the item into the map unsafely
//         let result = map.insert_directly(hash, item, pos);
//         assert_eq!(result, Ok(()));
//
//         // We expect that we can read the item directly from the map
//         assert_eq!(map.get(hash, |candidate| candidate.eq(&item)), Some(&item));
//     }
//
//     #[test]
//     pub fn can_directly_write_to_any_index_of_a_group_in_an_empty_map() {
//         for i in 0..Group::WIDTH {
//             let mut map = RawTable::<u64>::with_capacity(1);
//             let bucket_mask = map.get_bucket_mask();
//
//             let item = 1u64;
//             let hash = 1234u64;
//
//             // Calculate the initial position this item would normally be stored
//             let pos = (hash as usize) & bucket_mask;
//
//             // Increment the position to a different element in the same group
//             let pos = (pos + i) & bucket_mask;
//
//             // Insert the item into the map unsafely
//             let result = map.insert_directly(hash, item, pos);
//             assert_eq!(result, Ok(()));
//
//             // We expect that we can read the item directly from the map
//             assert_eq!(map.get(hash, |candidate| candidate.eq(&item)), Some(&item));
//         }
//     }
//
//     #[test]
//     pub fn can_directly_write_after_an_overflowed_group() {
//         let mut map = RawTable::<u64>::with_capacity(Group::WIDTH + 1);
//
//         // Write Group::WIDTH items with the same hash to the map
//         let hash = 1234u64;
//         for i in 0..Group::WIDTH {
//             let item = i as u64;
//             // Hasher shouldn't be used
//             map.insert(hash, item, |_| unreachable!());
//             assert_eq!(map.get(hash, |candidate| candidate.eq(&item)), Some(&item));
//         }
//
//         // Write an item with the same hash to the next bucket in the probe sequence
//         let bucket_mask = map.get_bucket_mask();
//
//         // Calculate the initial position this item would normally be stored
//         let pos = (hash as usize) & bucket_mask;
//         // Step to the next bucket in the probe sequence
//         let pos = (pos + Group::WIDTH) & bucket_mask;
//
//         // Insert the item into the map unsafely
//         let item = 101u64;
//         let result = map.insert_directly(hash, item, pos);
//         assert_eq!(result, Ok(()));
//
//         // We expect that we can read the item directly from the map
//         assert_eq!(map.get(hash, |candidate| candidate.eq(&item)), Some(&item));
//
//         // We should still be able to read the original items
//         for i in 0..Group::WIDTH {
//             let item = i as u64;
//             assert_eq!(map.get(hash, |candidate| candidate.eq(&item)), Some(&item));
//         }
//     }
// }
