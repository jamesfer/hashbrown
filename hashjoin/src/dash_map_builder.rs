use std::sync::Arc;
use dashmap::{DashMap, ReadOnlyView};
use tokio::sync::RwLock;
use crate::builder::utils::ClaimOnce;
use crate::lookup::Lookup;
use crate::utils::bypass_hasher::BypassHasher;

pub struct DashMapBuilder {
    parallelism: usize,
    state: Arc<Vec<ClaimOnce<Arc<(DashMap<u64, usize, BypassHasher>, RwLock<Vec<usize>>)>>>>,
    completed_state_sender: Arc<ClaimOnce<tokio::sync::broadcast::Sender<Arc<DashMapLookup>>>>,
    completed_state_receivers: Arc<Vec<ClaimOnce<tokio::sync::broadcast::Receiver<Arc<DashMapLookup>>>>>,
}

impl DashMapBuilder {
    pub fn new(parallelism: usize) -> Self {
        let state = Arc::new((DashMap::with_hasher(BypassHasher), RwLock::new(vec![0])));
        let state_instances = (0..parallelism).map(|_| ClaimOnce::new(state.clone())).collect();
        let sender = tokio::sync::broadcast::Sender::new(1);
        let receivers = (0..parallelism).map(|_| ClaimOnce::new(sender.subscribe())).collect();
        DashMapBuilder {
            parallelism,
            state: Arc::new(state_instances),
            completed_state_sender: Arc::new(ClaimOnce::new(sender)),
            completed_state_receivers: Arc::new(receivers),
        }
    }

    pub async fn run(&self, thread_index: usize, input: Vec<Vec<u64>>) -> Arc<DashMapLookup> {
        // Claim this thread's copy of the state
        let state = self.state[thread_index].claim().expect("State was already claimed");
        {
            let (map, overflow_buffer) = state.as_ref();
            for batch in input {
                let mut local_overflow_buffer = vec![0; batch.len()];

                // Allocate space for the overflow buffer
                let batch_offset = {
                    let overflow_buffer = &mut *overflow_buffer.write().await;
                    // Use the size of the overflow buffer as the current offset. Since the overflow
                    // buffer will start as a 1 length vector, the +1 we normally need to do is already
                    // included in this offset
                    let offset = overflow_buffer.len();
                    overflow_buffer.extend_from_slice(&local_overflow_buffer);
                    offset
                };

                // Write each hash to the shared map and the local overflow buffer
                for (index, hash) in batch.into_iter().enumerate() {
                    let global_index = batch_offset + index;
                    if let Some(existing) = map.insert(hash, global_index) {
                        local_overflow_buffer[index] = existing;
                    }
                }

                // Finally write the local overflow buffer to the shared buffer
                {
                    let overflow_buffer = &mut *overflow_buffer.write().await;
                    let reference = overflow_buffer[batch_offset..batch_offset + local_overflow_buffer.len()].as_mut();
                    reference.copy_from_slice(&local_overflow_buffer);
                }
            }
        }

        // Once all batches have been written, we need to finalise the state and share it between
        // all the threads for more efficient lookups.
        // This if condition will only become true on the last thread using the state
        if let Some((map, overflow_buffer)) = Arc::into_inner(state) {
            let sender = self.completed_state_sender.claim().expect("Sender was already claimed");
            let final_state = Arc::new(DashMapLookup {
                map: map.into_read_only(),
                overflow: overflow_buffer.into_inner(),
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
pub struct DashMapLookup {
    map: ReadOnlyView<u64, usize, BypassHasher>,
    overflow: Vec<usize>,
}

impl Lookup for DashMapLookup {
    fn get(&self, hash: u64) -> Vec<usize> {
        match self.map.get(&hash) {
            None => vec![],
            Some(index) => {
                let mut output = vec![index - 1];
                let mut next = self.overflow[*index];
                while next != 0 {
                    output.push(next - 1);
                    next = self.overflow[next];
                }
                output
            },
        }
    }
}
