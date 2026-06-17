// use std::cell::UnsafeCell;
// use std::ops::{Deref, DerefMut};
// use std::sync::{atomic, Arc};
// use std::sync::atomic::{AtomicUsize, Ordering};
// use std::vec::Vec;
// use tokio::sync::{broadcast, Barrier, Mutex};
// use crate::builder::global_index_tracker::{GlobalIndexTracker, Offset};
// use crate::builder::utils::UnsafeCellSendWrapper;
// use crate::HashMap;
// use crate::raw::{ProbeSeq, RawTable};
//
// #[derive(Debug, Clone)]
// struct IndexVal {
//     index: usize,
//     hash: u64,
// }
//
// #[derive(Debug, Clone)]
// struct Entry {
//     hash: u64,
//     index: usize,
//     end_of_overflow: usize,
// }
//
// #[derive(Debug, Clone)]
// struct OverflowIndex {
//     // thread_index: usize,
//     local_buffer_index: usize,
//     local_vec_index: usize,
//     global_vec_index: usize,
// }
//
// #[derive(Debug, Clone)]
// struct CombinedEntry {
//     hash: u64,
//     global_index: usize,
//     local_index: usize,
//     end_of_overflow: OverflowIndex,
// }
//
// #[derive(Debug, Clone)]
// struct SortedVec {
//     index: usize,
//     offset: usize,
//     entries: Vec<Entry>,
//     overflow: Vec<usize>,
// }
//
// fn handle_single_input_vec(
//     thread_index: usize,
//     global_offset: Offset,
//     local_buffer_index: usize,
//     input: Vec<u64>
// ) -> (Vec<CombinedEntry>, Vec<usize>) {
//
//     // Get and update the offset
//     let len = input.len();
//     let current_offset = global_offset.size;
//
//     let mut index_vals = input.into_iter()
//         // .zip(current_offset..current_offset + len)
//         .zip(0..len)
//         .map(|(value, index)| IndexVal { index, hash: value })
//         .collect::<Vec<_>>();
//
//     // Use sort unstable because it is marginally faster on random inputs and uses less
//     // memory
//     index_vals.sort_unstable_by_key(|IndexVal { hash: value, .. }| value);
//
//     // Write duplicates to the overflow buffer
//     let overflow = vec![0usize; index_vals.len()];
//     // TODO could count the number of duplicates to prevent reallocations
//     let mut dest = Vec::<CombinedEntry>::new();
//     let mut prev_item = Entry {
//         hash: index_vals[0].hash,
//         index: index_vals[0].index,
//         end_of_overflow: index_vals[0].index,
//     };
//     for item in index_vals.into_iter().skip(1) {
//         // Check if item is the same as previous
//         if item.hash == prev_item.hash {
//             // This is a duplicate, write the previous value to the overflow buffer
//             let stored_index = prev_item.index + current_offset + 1;
//             overflow[item.index] = stored_index;
//             prev_item.index = item.index;
//         } else {
//             // This is a different value, write the previous value to the destination and
//             // set the previous value to this item
//             let next_entry = Entry {
//                 hash: item.hash,
//                 index: item.index,
//                 end_of_overflow: item.index,
//             };
//             let completed_entry = std::mem::replace(&mut prev_item, next_entry);
//             dest.push(CombinedEntry {
//                 hash: completed_entry.hash,
//                 global_index: completed_entry.index + current_offset,
//                 local_index: completed_entry.index,
//                 end_of_overflow: OverflowIndex {
//                     local_buffer_index,
//                     local_vec_index: completed_entry.end_of_overflow,
//                     global_vec_index: completed_entry.index + current_offset + 1,
//                 },
//             });
//         }
//     }
//     // At the end of the loop, we need to write the last item to the destination
//     dest.push(CombinedEntry {
//         hash: prev_item.hash,
//         global_index: prev_item.index + current_offset,
//         local_index: prev_item.index,
//         end_of_overflow: OverflowIndex {
//             local_buffer_index,
//             local_vec_index: prev_item.end_of_overflow,
//             global_vec_index: prev_item.index + current_offset + 1,
//         },
//     });
//
//     (dest, overflow)
//     // Some(SortedVec {
//     //     index: vec_index,
//     //     offset: current_offset,
//     //     entries: dest,
//     //     overflow,
//     // })
// }
//
// fn merge_locally(
//     mut overflow_buffers: &mut Vec<OrderedOverflowBuffer>,
//     vecs: Vec<Vec<CombinedEntry>>,
// ) -> Vec<CombinedEntry> {
//     // let total_len = vecs.iter()
//     //     .map(|vec| vec.len())
//     //     .sum::<usize>();
//     // let mut destination = Vec::with_capacity(total_len);
//
//     // Copy each of the entries into the destination
//     // TODO avoid clone
//     // for (vec_index, vec) in vecs.into_iter().enumerate() {
//     //     let total_vec_index = vec_index + chunk_index * 8;
//     //     for entry in vec.into_iter() {
//     //         destination.push(CombinedEntry {
//     //             hash: entry.hash,
//     //             global_index: entry.global_index,
//     //             local_index: entry.index,
//     //             end_of_overflow: OverflowIndex {
//     //                 thread_index,
//     //                 buffer_index: total_vec_index,
//     //                 vec_index: entry.end_of_overflow,
//     //             },
//     //         });
//     //     }
//     // }
//
//     let mut destination = vecs.into_iter().flatten().collect::<Vec<_>>();
//
//     // Use stable sort because it is designed to find existing patterns in the data
//     // TODO check if unstable sort has the same behaviour
//     destination.sort_by_key(|x| x.hash);
//
//     // // TODO Handle duplicates and write to overflow
//     // let overflow = vec![0usize; index_vals.len()];
//     // let is_duplicate = vec![false; index_vals.len()];
//     let mut dest2 = Vec::<CombinedEntry>::new(); // TODO could count the number of duplicates to prevent reallocations
//     let mut prev_item = destination[0].clone();
//     for item in destination.into_iter().skip(1) {
//         // Check if item is the same as previous
//         if item.hash == prev_item.hash {
//             // This is a duplicate, insert the current item at the end of the previous
//             // chain, then update the end of the previous item to be this items end
//             let mut overflow = &mut overflow_buffers[prev_item.end_of_overflow.local_buffer_index];
//             let stored_index = prev_item.global_index + 1;
//             overflow.buffer[prev_item.end_of_overflow.local_vec_index] = stored_index;
//             prev_item.end_of_overflow = item.end_of_overflow;
//         } else {
//             // This is a different value, write the previous value to the destination and
//             // set the previous value to this item
//             dest2.push(std::mem::replace(&mut prev_item, item));
//         }
//     }
//     // At the end of the loop, we need to write the last item to the destination
//     dest2.push(prev_item);
//
//     dest2
// }
//
// #[derive(Debug, Clone)]
// struct OrderedOverflowBuffer {
//     global_index: usize,
//     global_offset: usize,
//     buffer: Vec<usize>,
// }
//
// // TODO this algorithm uses fat vector items, where all the details for an item is
// //   stored inside the vector and is moved each time the vector is sorted. This avoids
// //   having to lookup the real values in a separate array each time we need the value.
// //   To be honest, I have no idea if it is better to avoid keeping separate lookup
// //   arrays (which this algorithm does), or it is better to reduce the size of the
// //   arrays being sorted to reduce the cost of data copying.
// async fn accumulate_locally(
//     thread_index: usize,
//     global_index_tracker: &GlobalIndexTracker,
//     inputs: Vec<Vec<u64>>,
// ) -> (Vec<CombinedEntry>, Vec<OrderedOverflowBuffer>) {
//     // Local part. Accumulate the input into a large sorted array
//     // Each input vector is considered completely random, so they are sorted using the unstable
//     // algorithm. Then many inputs are concatenated together and sorted using the stable sort
//     // algorithm which has special cases for partially sorted inputs. We assume this is faster
//     // than trying to create a merge sort algorithm ourselves.
//
//     let mut overflow_buffers = Vec::<OrderedOverflowBuffer>::new();
//     let mut local_sorted = Vec::<Vec<CombinedEntry>>::new();
//
//     // Process all the input buffers independently
//     for input in inputs.into_iter() {
//         let global_offset = global_index_tracker.allocate(input.len()).await;
//         let (entries, overflow) = handle_single_input_vec(
//             thread_index,
//             global_offset.clone(),
//             overflow_buffers.len(),
//             input,
//         );
//         local_sorted.push(entries);
//         overflow_buffers.push(OrderedOverflowBuffer{
//             global_offset: global_offset.size,
//             global_index: global_offset.index,
//             buffer: overflow,
//         });
//     }
//
//     // Merge the sorted arrays when there are 8 of them together. This number is chosen arbitrarily.
//     // We do this in chunks to help remove large numbers of duplicate values earlier on.
//     let mut chunks = local_sorted;
//     while chunks.len() > 1 {
//         chunks = chunks.chunks(8)
//             .enumerate()
//             .map(|(chunk_index, vecs)| -> Vec<CombinedEntry> {
//                 if vecs.len() == 1 {
//                     // TODO avoid clone
//                     return vecs[0].clone();
//                 }
//
//                 // TODO avoid clone
//                 merge_locally(&mut overflow_buffers, vecs.to_vec())
//             })
//             .collect::<Vec<_>>();
//     }
//     assert_eq!(chunks.len(), 1, "Chunks should have exactly one element");
//     let full_sorted = chunks.pop().unwrap();
//
//     (full_sorted, overflow_buffers)
// }
//
// // TODO implement copy on other traits?
// #[derive(Debug, Clone, Copy)]
// struct FinalEntry {
//     hash: u64,
//     global_index: usize,
//     end_of_chain: usize,
// }
//
// async fn sort_globally(
//     thread_index: usize,
//     local_sorted: Vec<CombinedEntry>,
//     overflow_buffers: Vec<OrderedOverflowBuffer>,
//     input_sizes: &UnsafeCellSendWrapper<Vec<usize>>,
//     overflow_size: &Mutex<usize>,
//     wait_until_all_sizes_written: &Barrier,
//     destination_buffer_sender: Option<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<FinalEntry>>>>>,
//     mut destination_buffer_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<FinalEntry>>>>,
//     overflow_buffer_sender: Option<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>,
//     mut overflow_buffer_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>,
//     destination_map_sender: Option<broadcast::Sender<Arc<UnsafeCellSendWrapper<RawTable<usize>>>>>,
//     mut destination_map_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<RawTable<usize>>>>,
//     wait_until_all_destination_buffers_written: &Barrier,
//     wait_until_all_destination_buffer_sorted: &Barrier,
// ) -> (Arc<UnsafeCellSendWrapper<Vec<FinalEntry>>>, Arc<UnsafeCellSendWrapper<Vec<usize>>>, Arc<UnsafeCellSendWrapper<RawTable<usize>>>) {
//     // Write the size of this threads input to the shared vec
//     unsafe { &mut *input_sizes.get() }[thread_index] = local_sorted.len();
//     // unsafe { &* self.overflow_sizes.get() }[thread_index]
//     {
//         // TODO maybe this could use an atomic variable instead. But I don't know how to use
//         //   fences
//         let mut overflow_size = overflow_size.lock().await;
//         *overflow_size += overflow_buffers.iter()
//             .map(|overflow| overflow.buffer.len())
//             .sum::<usize>();
//     }
//     wait_until_all_sizes_written.wait().await;
//
//     // Allocate the destination buffer if this thread is responsible for it
//     let total_size = unsafe { &mut *input_sizes.get() }.iter().sum::<usize>();
//     if let Some(destination_buffer_sender) = destination_buffer_sender {
//         let destination_buffer = vec![FinalEntry { global_index: 0, end_of_chain: 0, hash: 0 }; total_size];
//         destination_buffer_sender.send(Arc::new(UnsafeCellSendWrapper::new(UnsafeCell::new(destination_buffer))))
//             .map_err(|_| "Failed to send destination buffer")
//             .unwrap();
//     }
//     if let Some(overflow_buffer_sender) = overflow_buffer_sender {
//         // We can use try lock since we must be the only thread reading the overflow size at
//         // this point thanks to the barrier above
//         let total_size = { *overflow_size.try_lock().unwrap() };
//         let overflow_buffer = vec![0usize; total_size];
//         overflow_buffer_sender.send(Arc::new(UnsafeCellSendWrapper::new(UnsafeCell::new(overflow_buffer))))
//             .map_err(|_| "Failed to send overflow buffer")
//             .unwrap();
//     }
//     if let Some(destination_map_sender) = destination_map_sender {
//         let destination_map = RawTable::with_capacity(total_size);
//         destination_map_sender.send(Arc::new(UnsafeCellSendWrapper::new(UnsafeCell::new(destination_map))))
//             .map_err(|_| "Failed to send destination map")
//             .unwrap();
//     }
//
//     // Write all of our local data to the two shared buffers
//     let destination_buffer = destination_buffer_receiver.recv().await
//         .map_err(|_| "Failed to receive destination buffer")
//         .unwrap();
//     let mut mutable_destination_buffer = unsafe { &mut *destination_buffer.get() };
//
//     // Add up the size of all previous threads to find the location where we will write
//     let input_sizes = input_sizes.get();
//     let destination_index = input_sizes[..thread_index].iter().sum::<usize>();
//     for (local_index, entry) in local_sorted.into_iter().enumerate() {
//         mutable_destination_buffer[destination_index + local_index] = FinalEntry {
//             hash: entry.hash,
//             global_index: entry.global_index,
//             end_of_chain: entry.end_of_overflow.global_vec_index,
//         };
//     }
//
//     let global_overflow_buffer = overflow_buffer_receiver.recv().await
//         .map_err(|_| "Failed to receive global overflow buffer")
//         .unwrap();
//     let mut mutable_global_overflow_buffer = unsafe { &mut *global_overflow_buffer.get() };
//
//     // Write each overflow buffer to the correct location based on the buffer's offset
//     for overflow_buffer in overflow_buffers.into_iter() {
//         mutable_global_overflow_buffer[overflow_buffer.global_offset..overflow_buffer.global_offset + overflow_buffer.buffer.len()]
//             .copy_from_slice(overflow_buffer.buffer.as_slice());
//     }
//
//     // Wait for all the copying to complete
//     wait_until_all_destination_buffers_written.wait().await;
//
//     // If we are the first thread (chosen arbitrarily), sort the destination buffer
//     // TODO perform this cooperatively
//     if thread_index == 0 {
//         mutable_destination_buffer.sort_by_key(|entry| entry.hash);
//     }
//     // TODO maybe don't use a buffer here just incase thread 0 is faster than the others for
//     //   some reason
//     wait_until_all_destination_buffer_sorted.wait().await;
//
//     let destination_map = destination_map_receiver.recv().await
//         .map_err(|_| "Failed to receive destination map")
//         .unwrap();
//
//     (destination_buffer, global_overflow_buffer, destination_map)
// }
//
// fn write_to_destination_map(
//     parallelism: usize,
//     thread_index: usize,
//     sorted_entries: &Vec<FinalEntry>,
//     overflow: Arc<UnsafeCellSendWrapper<Vec<usize>>>,
//     destination_map: Arc<UnsafeCellSendWrapper<RawTable<usize>>>,
// ) {
//     let mutable_destination_map = unsafe { &mut *destination_map.get() };
//     let mutable_overflow = unsafe { &mut *overflow.get() };
//
//     let buckets = mutable_destination_map.buckets();
//     assert!(buckets.is_power_of_two(), "Buckets must be a power of two");
//     let bucket_mask = buckets - 1;
//
//     // Determine which indices this thread is responsible for
//     let ceil_values_per_thread = sorted_entries.len().div_ceil(parallelism);
//     let remainder = sorted_entries.len() % parallelism;
//     let start_index = thread_index * ceil_values_per_thread - thread_index.saturating_sub(remainder);
//     let end_index = start_index + ceil_values_per_thread - (if thread_index < remainder { 1 } else { 0 });
//     let owned_range = start_index..end_index;
//
//     // Allocate a buffer to track which buckets in the destination map have been written to
//     let occupied = vec![false; buckets];
//
//     // Loop over each sorted entry, and write it to the destination map when we are the owner
//     let mut previous_entry = FinalEntry {
//         hash: 0,
//         global_index: 0,
//         end_of_chain: 0,
//     };
//     for (index, entry) in sorted_entries.iter().enumerate() {
//         let owned = owned_range.contains(&index);
//         let hash = entry.hash;
//         if hash == previous_entry.hash {
//             if owned {
//                 mutable_overflow[previous_entry.end_of_chain] = entry.global_index + 1;
//             }
//             previous_entry.end_of_chain = entry.end_of_chain;
//             continue;
//         }
//         previous_entry = *entry;
//
//         let probe_seq = ProbeSeq {
//             pos: hash as usize & bucket_mask,
//             stride: 0,
//         };
//
//         // Check the first 8 entries in the occupied buffer to find the first one available
//         for group_pos in 0usize..8 {
//             let index = (probe_seq.pos + group_pos) & bucket_mask;
//             if !std::mem::replace(&mut occupied[index], true) {
//                 if owned {
//                     mutable_destination_map.insert_directly(hash, entry.global_index, index);
//                 }
//                 break;
//             }
//         }
//     }
// }
//
// // All of these should actually be function inputs
// pub struct SortedMerger {
//     parallelism: usize,
//     global_index_tracker: GlobalIndexTracker,
//     input_sizes: UnsafeCellSendWrapper<Vec<usize>>,
//     overflow_size: Mutex<usize>,
//     wait_until_all_sizes_written: Barrier,
//
//     destination_buffer_sender: Option<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<FinalEntry>>>>>,
//     destination_buffer_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<FinalEntry>>>>,
//     overflow_buffer_sender: Option<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>,
//     overflow_buffer_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>,
//     destination_map_sender: Option<broadcast::Sender<Arc<UnsafeCellSendWrapper<RawTable<usize>>>>>,
//     destination_map_receiver: broadcast::Receiver<Arc<UnsafeCellSendWrapper<RawTable<usize>>>>,
//     wait_until_all_destination_buffers_written: Barrier,
//     wait_until_all_destination_buffer_sorted: Barrier,
// }
//
// impl SortedMerger {
//     async fn run(
//         self,
//         thread_index: usize,
//         input: Vec<Vec<u64>>,
//     ) {
//         // Run the local part to completion first
//         let (local_sorted, overflow_buffers) = accumulate_locally(
//             thread_index,
//             &self.global_index_tracker,
//             input,
//         ).await;
//
//         let (sorted_entries, overflow, destination_map) = sort_globally(
//             thread_index,
//             local_sorted,
//             overflow_buffers,
//             &self.input_sizes,
//             &self.overflow_size,
//             &self.wait_until_all_sizes_written,
//             self.destination_buffer_sender,
//             self.destination_buffer_receiver,
//             self.overflow_buffer_sender,
//             self.overflow_buffer_receiver,
//             self.destination_map_sender,
//             self.destination_map_receiver,
//             &self.wait_until_all_destination_buffers_written,
//             &self.wait_until_all_destination_buffer_sorted,
//         ).await;
//
//         // Now all the threads can write to the final map in parallel.
//         write_to_destination_map(
//             self.parallelism,
//             thread_index,
//             unsafe { &* sorted_entries.get() },
//             overflow,
//             destination_map
//         )
//
//         ()
//     }
//
// }
