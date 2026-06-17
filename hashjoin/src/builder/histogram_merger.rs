// use std::cell::UnsafeCell;
// use std::sync::Arc;
// use std::vec::Vec;
// use tokio::sync::broadcast;
// use crate::builder::build_me::{write_local_histograms_to_shared_map, LocalHistogram};
// use crate::builder::global_index_tracker::GlobalIndexTracker;
// use crate::builder::histogram_local_accumulate::locally_accumulate_histograms;
// use crate::builder::utils::{ClaimOnce, UnsafeCellSendWrapper};
// use crate::HashTable;
//
// pub struct HistogramMerger {
//     parallelism: usize,
//     histogram_bucket_count: usize,
//     destination_bucket_count: usize,
//     global_index_tracker: GlobalIndexTracker,
//     completed_local_histogram_senders: Vec<ClaimOnce<broadcast::Sender<Arc<LocalHistogram>>>>,
//     completed_local_histogram_receivers: Vec<ClaimOnce<broadcast::Receiver<Arc<LocalHistogram>>>>,
//     local_bucket_size_senders: Vec<ClaimOnce<broadcast::Sender<usize>>>,
//     local_bucket_size_receivers: Vec<ClaimOnce<broadcast::Receiver<usize>>>,
//     destination_bucket_size_vector_sender: ClaimOnce<broadcast::Sender<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>,
//     destination_bucket_size_vector_receivers: Vec<ClaimOnce<broadcast::Receiver<Arc<UnsafeCellSendWrapper<Vec<usize>>>>>>,
//     destination_bucket_sizes_written_senders: Vec<ClaimOnce<broadcast::Sender<()>>>,
//     destination_bucket_sizes_written_receivers: Vec<ClaimOnce<broadcast::Receiver<()>>>,
//     destination_map_sender: ClaimOnce<broadcast::Sender<Arc<UnsafeCellSendWrapper<HashTable<usize>>>>>,
//     destination_map_receivers: Vec<ClaimOnce<broadcast::Receiver<Arc<UnsafeCellSendWrapper<HashTable<usize>>>>>>,
//     destination_map_written_senders: Vec<ClaimOnce<broadcast::Sender<()>>>,
//     destination_map_written_receivers: Vec<ClaimOnce<broadcast::Receiver<()>>>,
// }
//
// impl HistogramMerger {
//     pub fn new(
//         parallelism: usize,
//         histogram_bucket_count: usize,
//         destination_bucket_count: usize,
//     ) -> Self {
//         let completed_local_histogram_sender = broadcast::Sender::new(100);
//         let completed_local_histogram_senders = (0..parallelism)
//             .map(|_| ClaimOnce::new(completed_local_histogram_sender.clone()))
//             .collect();
//         let completed_local_histogram_receivers = (0..parallelism)
//             .map(|_| ClaimOnce::new(completed_local_histogram_sender.subscribe()))
//             .collect();
//         let local_bucket_size_sender = broadcast::Sender::new(100);
//         let local_bucket_size_senders = (0..parallelism)
//             .map(|_| ClaimOnce::new(local_bucket_size_sender.clone()))
//             .collect();
//         let local_bucket_size_receivers = (0..parallelism)
//             .map(|_| ClaimOnce::new(local_bucket_size_sender.subscribe()))
//             .collect();
//         let destination_bucket_size_vector_sender = broadcast::Sender::new(100);
//         let destination_bucket_size_vector_receivers = (0..parallelism)
//             .map(|_| ClaimOnce::new(destination_bucket_size_vector_sender.subscribe()))
//             .collect();
//         let destination_bucket_size_vector_sender = ClaimOnce::new(destination_bucket_size_vector_sender);
//         let destination_bucket_sizes_written_sender = broadcast::Sender::new(100);
//         let destination_bucket_sizes_written_senders = (0..parallelism)
//             .map(|_| ClaimOnce::new(destination_bucket_sizes_written_sender.clone()))
//             .collect();
//         let destination_bucket_sizes_written_receivers = (0..parallelism)
//             .map(|_| ClaimOnce::new(destination_bucket_sizes_written_sender.subscribe()))
//             .collect();
//         let destination_map_sender = broadcast::Sender::new(100);
//         let destination_map_receivers = (0..parallelism)
//             .map(|_| ClaimOnce::new(destination_map_sender.subscribe()))
//             .collect();
//         let destination_map_sender = ClaimOnce::new(destination_map_sender);
//         let destination_map_written_sender = broadcast::Sender::new(100);
//         let destination_map_written_senders = (0..parallelism)
//             .map(|_| ClaimOnce::new(destination_map_written_sender.clone()))
//             .collect();
//         let destination_map_written_receivers = (0..parallelism)
//             .map(|_| ClaimOnce::new(destination_map_written_sender.subscribe()))
//             .collect();
//
//         Self {
//             parallelism,
//             histogram_bucket_count,
//             destination_bucket_count,
//             global_index_tracker: GlobalIndexTracker::new(),
//             completed_local_histogram_senders,
//             completed_local_histogram_receivers,
//             local_bucket_size_senders,
//             local_bucket_size_receivers,
//             destination_bucket_size_vector_sender,
//             destination_bucket_size_vector_receivers,
//             destination_bucket_sizes_written_senders,
//             destination_bucket_sizes_written_receivers,
//             destination_map_sender,
//             destination_map_receivers,
//             destination_map_written_senders,
//             destination_map_written_receivers,
//         }
//     }
//
//     pub async fn run(&self, thread_index: usize, inputs: Vec<Vec<u64>>) -> (Arc<UnsafeCellSendWrapper<HashTable<usize>>>, Vec<usize>) {
//         // Build the histogram from the inputs this thread receives
//         let (buckets, overflows) = locally_accumulate_histograms(
//             thread_index,
//             self.histogram_bucket_count,
//             &self.global_index_tracker,
//             inputs,
//         ).await;
//
//         let arc = Arc::new(UnsafeCellSendWrapper::new(UnsafeCell::new(overflows)));
//         let local_histogram = LocalHistogram {
//             buckets: buckets.into_iter().map(|bucket| ClaimOnce::new(bucket)).collect(),
//             overflows: arc.clone(),
//         };
//         let table = write_local_histograms_to_shared_map(
//             thread_index,
//             self.parallelism,
//             self.destination_bucket_count,
//             local_histogram,
//             self.completed_local_histogram_senders.get(thread_index).unwrap().claim().unwrap(),
//             self.completed_local_histogram_receivers.get(thread_index).unwrap().claim().unwrap(),
//             self.local_bucket_size_senders.get(thread_index).unwrap().claim().unwrap(),
//             self.local_bucket_size_receivers.get(thread_index).unwrap().claim().unwrap(),
//             if thread_index == 0 {
//                 Some(self.destination_bucket_size_vector_sender.claim().unwrap())
//             } else {
//                 None
//             },
//             self.destination_bucket_size_vector_receivers.get(thread_index).unwrap().claim().unwrap(),
//             self.destination_bucket_sizes_written_senders.get(thread_index).unwrap().claim().unwrap(),
//             self.destination_bucket_sizes_written_receivers.get(thread_index).unwrap().claim().unwrap(),
//             // We would like the thread that allocates the destination map to be different from the
//             // thread that builds the destination bucket size vector
//             if thread_index + 1 == 1 {
//                 Some(self.destination_map_sender.claim().unwrap())
//             } else {
//                 None
//             },
//             self.destination_map_receivers.get(thread_index).unwrap().claim().unwrap(),
//             self.destination_map_written_senders.get(thread_index).unwrap().claim().unwrap(),
//             self.destination_map_written_receivers.get(thread_index).unwrap().claim().unwrap(),
//         ).await;
//
//         // Process all of the overflow vecs
//         // TODO merge these in the above function
//         let mut overflow_vecs = unsafe { arc.as_ref().get().as_ref() }.unwrap()
//             .iter()
//             .cloned()
//             .collect::<Vec<_>>();
//         overflow_vecs.sort_by_key(|buffer| buffer.global_buffer_index);
//
//         let overflow = std::iter::once(vec![0usize])
//             .chain(overflow_vecs.into_iter().map(|buffer| buffer.buffer_data))
//             .flatten()
//             .collect::<Vec<_>>();
//
//         (table, overflow)
//     }
//
//     pub fn lookup(
//         table: &UnsafeCellSendWrapper<HashTable<usize>>,
//         overflow: &Vec<usize>,
//         hash: u64,
//     ) -> Vec<usize> {
//         let table = unsafe { &*table.get() };
//         let initial = table.find(hash, |_| true);
//         println!("looking up {}, initial: {:?}, size {}", hash, initial, table.len());
//         match initial {
//             None => vec![],
//             Some(index) => {
//                 let mut index = *index;
//                 let mut output = vec![];
//                 while index != 0 {
//                     output.push(index - 1);
//                     index = overflow[index];
//                 }
//                 output
//             }
//         }
//     }
// }
//
//
// #[cfg(test)]
// mod tests {
//     use crate::builder::histogram_merger::HistogramMerger;
//
//     #[tokio::test(flavor = "multi_thread")]
//     pub async fn test() {
//         let merger = HistogramMerger::new(1, 16, 16);
//         let (table, overflow) = merger.run(0, vec![vec![1, 2, 3, 4, 5, 6], vec![6, 7, 8, 9, 10]]).await;
//         for i in 1usize..=6 {
//             assert_eq!(HistogramMerger::lookup(table.as_ref(), &overflow, i as u64), vec![i - 1]);
//         }
//     }
// }
