use std::sync::Arc;
use tokio::sync::RwLock;
use hashbrown::hash_table::Entry;
use hashbrown::HashTable;
use crate::builder::utils::ClaimOnce;
use crate::lookup::Lookup;

pub struct MutexMapBuilder {
    parallelism: usize,
    shared_state: Arc<Vec<ClaimOnce<Arc<RwLock<(HashTable<(u64, usize)>, Vec<usize>)>>>>>,
    completed_state_sender: Arc<ClaimOnce<tokio::sync::broadcast::Sender<Arc<MutexMapLookup>>>>,
    completed_state_receivers: Arc<Vec<ClaimOnce<tokio::sync::broadcast::Receiver<Arc<MutexMapLookup>>>>>,
}

impl MutexMapBuilder {
    pub fn new(parallelism: usize) -> Self {
        let state = Arc::new(RwLock::new((HashTable::new(), vec![0])));
        let state_instances = (0..parallelism).map(|_| ClaimOnce::new(state.clone())).collect();
        let sender = tokio::sync::broadcast::Sender::new(1);
        let receivers = (0..parallelism).map(|_| ClaimOnce::new(sender.subscribe())).collect();
        MutexMapBuilder {
            parallelism,
            shared_state: Arc::new(state_instances),
            completed_state_sender: Arc::new(ClaimOnce::new(sender)),
            completed_state_receivers: Arc::new(receivers),
        }
    }

    pub async fn run(&self, thread_index: usize, input: Vec<Vec<u64>>) -> Arc<MutexMapLookup> {
        // Claim this thread's copy of the state
        let state = self.shared_state[thread_index].claim().expect("State was already claimed");

        for batch in input {
            // Allocate space for the overflow buffer
            let batch_offset = {
                let (_, overflow_buffer) = &mut *state.write().await;
                // Use the size of the overflow buffer as the current offset. Since the overflow
                // buffer will start as a 1 length vector, the +1 we normally need to do is already
                // included in this offset
                let offset = overflow_buffer.len();
                overflow_buffer.resize(offset + batch.len(), 0);
                offset
            };

            for (index, hash) in batch.into_iter().enumerate() {
                let (map, overflow) = &mut *state.write().await;
                let insertion_index = index + batch_offset;
                match map.entry(hash, |(k, _)| k == &hash, |(k, _)| *k) {
                    Entry::Vacant(entry) => {
                        entry.insert((hash, insertion_index));
                    },
                    Entry::Occupied(mut entry) => {
                        let existing = std::mem::replace(&mut entry.get_mut().1, insertion_index);
                        overflow[insertion_index] = existing;
                    }
                }
            }
        }

        // Once all batches have been written, we need to finalise the state and share it between
        // all the threads for more efficient lookups.
        // This if condition will only become true on the last thread using the state
        if let Some(lock) = Arc::into_inner(state) {
            let sender = self.completed_state_sender.claim().expect("Sender was already claimed");
            let (map, overflow_buffer) = lock.into_inner();
            let final_state = Arc::new(MutexMapLookup {
                map,
                overflow: overflow_buffer,
            });
            sender.send(Arc::clone(&final_state)).expect("Failed to send final state");
            final_state
        } else {
            let mut receiver = self.completed_state_receivers[thread_index].claim().expect("Receiver was already claimed");
            receiver.recv().await.expect("Failed to receive final state")
        }
    }
}

#[derive(Debug)]
pub struct MutexMapLookup {
    map: HashTable<(u64, usize)>,
    overflow: Vec<usize>,
}

impl Lookup for MutexMapLookup {
    fn get(&self, hash: u64) -> Vec<usize> {
        match self.map.find(hash, |(k, _)| k == &hash) {
            None => vec![],
            Some((_, index)) => {
                let mut output = vec![index - 1];
                let mut next = self.overflow[*index];
                while next != 0 {
                    output.push(next - 1);
                    next = self.overflow[next];
                }
                output
            }
        }
    }
}
