use crate::new_map_2::atomic::AsAtomic;
use crate::new_map_2::write_notify_cell::WriteNotifyCell;
use crate::new_map_3::fixed_table::{ReadOnlyFixedTable, WritableFixedTable};
use crate::new_map_3::group::Group8;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::atomic::{fence, AtomicI64, AtomicPtr, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

type Group = Group8;

// Just a data wrapper around a fixed table with some other information
struct InnerTable<V> {
    // TODO change to smaller type
    generation: u32,
    // consumed_capacity: AtomicUsize,
    // remaining_capacity: AtomicI64,
    total_capacity_of_previous_tables: usize,
    table: WritableFixedTable<V, Group>,
}

impl <V> InnerTable<V>
where V: Default + Copy + AsAtomic + PartialEq + 'static
{
    fn new(generation: u32, capacity: usize, total_capacity_of_previous_tables: usize) -> Self {
        // TODO check capacity is greater than 0 and less than a maximum value. Keeping in mind that
        //  the maximum size of the table must fit in an i64
        let table = WritableFixedTable::with_capacity(capacity);
        // Actual capacity of the table may be greater than the value we requested
        let actual_capacity = table.capacity();
        Self {
            generation,
            // consumed_capacity: AtomicUsize::new(0),
            // remaining_capacity: AtomicI64::new(actual_capacity as i64),
            total_capacity_of_previous_tables,
            table,
        }
    }
}

struct SharedTableBuilder<V> {
    current_table: AtomicPtr<InnerTable<V>>,
    full_tables: Mutex<Vec<Box<InnerTable<V>>>>,
    total_size_during_compaction: AtomicUsize,
    migration_state: AtomicU64,
    compaction_elements: WriteNotifyCell<Arc<Compactor<V>>>,
}

struct Compactor<V> {
    destination_table: WritableFixedTable<V, Group>,
    source_tables: Vec<WritableFixedTable<V, Group>>,
    next_table_counter: AtomicUsize,
    completed_table_write_cell: WriteNotifyCell<Arc<ReadOnlyFixedTable<V, Group>>>
}

#[derive(Clone)]
pub struct WriteOnlyTable<V> {
    write_state: Arc<SharedTableBuilder<V>>,
    current_table_cache: *mut InnerTable<V>,
    writes_to_current_table: usize,
    total_writes: usize,
    // loaded_tags_buffer: [u8; 16],
    // match_output_buffer: [u8; 16],
    waiting_for_in_progress_migration: u64,
    waiting_for_previous_migration: u64,
    time_wasted_due_to_full_table: u64,
    failed_writes: usize,
}

impl<V> WriteOnlyTable<V> {
    pub fn failed_writes(&self) -> usize {
        self.failed_writes
    }
}

unsafe impl <V: Sync> Sync for WriteOnlyTable<V> {}
unsafe impl <V: Send> Send for WriteOnlyTable<V> {}

impl <V> WriteOnlyTable<V>
where V: Default + Copy + AsAtomic + PartialEq + 'static
{
    pub fn new() -> Self {
        let compaction_elements = WriteNotifyCell::new();
        // let compaction_elements_reader = compaction_elements.get_reader();
        Self {
            write_state: Arc::new(SharedTableBuilder {
                current_table: AtomicPtr::new(Box::into_raw(Box::new(InnerTable::new(0, 16, 0)))),
                full_tables: Mutex::new(Vec::new()),
                total_size_during_compaction: AtomicUsize::new(0),
                migration_state: AtomicU64::new(0),
                compaction_elements,
            }),
            // loaded_tags_buffer: [0; 16],
            // match_output_buffer: [0; 16],
            current_table_cache: std::ptr::null_mut(),
            writes_to_current_table: 0,
            total_writes: 0,
            waiting_for_in_progress_migration: 0,
            waiting_for_previous_migration: 0,
            time_wasted_due_to_full_table: 0,
            failed_writes: 0,
        }
    }

    pub async fn compact(self) -> Arc<ReadOnlyFixedTable<V, Group>> {
        // print summary
        // println!("Waiting for in progress migration: {} ns, waiting for previous migration: {} ns, wasted due to full table: {} ns", self.waiting_for_in_progress_migration, self.waiting_for_previous_migration, self.time_wasted_due_to_full_table);

        // Update the remaining capacity of the current table
        // if !self.current_table_cache.is_null() {
        //     let current_table = unsafe { &*self.current_table_cache };
        //     current_table.consumed_capacity.fetch_add(self.writes_to_current_table, Ordering::Release);
        // } else {
        //     assert_eq!(self.writes_to_current_table, 0);
        // }

        self.write_state.total_size_during_compaction.fetch_add(self.total_writes, Ordering::Release);

        let compaction_elements_reader = self.write_state.compaction_elements.get_reader();
        let compaction_elements = if let Some(write_state) = Arc::into_inner(self.write_state) {
            drop(compaction_elements_reader);

            // This thread is the last writer to the table
            // Step 1. Check if we need to allocate a new table to hold all the final results, or
            // if the data will fit in the existing largest table.
            let current_table = unsafe { Box::from_raw(write_state.current_table.into_inner()) };
            let all_tables: Vec<_> = write_state.full_tables.into_inner().unwrap();

            let current_capacity = current_table.table.capacity();
            let total_size = write_state.total_size_during_compaction.load(Ordering::Acquire);
            let (destination_table, source_tables) = if total_size > current_capacity {
                // Need to allocate new table
                let new_destination_table = WritableFixedTable::with_capacity(total_size);

                let source_tables: Vec<_> = all_tables.into_iter()
                    .map(|table| table.table)
                    .chain(std::iter::once(current_table.table))
                    .collect();
                (new_destination_table, source_tables)
            } else {
                (current_table.table, all_tables.into_iter().map(|table| table.table).collect())
            };

            let compaction_elements = Arc::new(Compactor {
                destination_table,
                source_tables,
                next_table_counter: AtomicUsize::new(0),
                completed_table_write_cell: WriteNotifyCell::new(),
            });
            write_state.compaction_elements.write(Arc::clone(&compaction_elements));

            compaction_elements
        } else {
            // Wait for the compaction state to be ready
            let compaction_elements = Arc::clone(compaction_elements_reader.read().await);
            drop(compaction_elements_reader);

            compaction_elements
        };

        // Step 2. Perform the compaction in parallel
        let destination_table = &compaction_elements.destination_table;
        let source_count = compaction_elements.source_tables.len();
        let mut source_index = compaction_elements.next_table_counter.fetch_add(1, Ordering::Relaxed);
        while source_index < source_count {
            let source_table = &compaction_elements.source_tables[source_index];
            for pair in source_table.entries() {
                // println!("Migrating value {:?} with hash {} from table index {}", pair.1, pair.0, source_index);
                destination_table.insert(pair.0, pair.1.clone()).unwrap();
            }

            source_index = compaction_elements.next_table_counter.fetch_add(1, Ordering::Relaxed);
        }

        // Step 3. Wait for all compaction to complete
        let read = compaction_elements.completed_table_write_cell.get_reader();
        let read_table = if let Some(compaction_elements) = Arc::into_inner(compaction_elements) {
            drop(read);

            let read_table = Arc::new(compaction_elements.destination_table.to_read_only());
            compaction_elements.completed_table_write_cell.write(Arc::clone(&read_table));
            read_table
        } else {
            Arc::clone(read.read().await)
        };

        read_table
    }

    // #[inline]
    pub fn insert(&mut self, hash: u64, value: V) -> Option<V> {
        // Attempt to write to the cached table first
        if !self.current_table_cache.is_null() {
            let current_table = unsafe { &*self.current_table_cache };
            if let Ok(output) = current_table.table.insert(hash, value) {
                if output == None {
                    // self.writes_to_current_table += 1;
                    self.total_writes += 1;
                    // current_table.remaining_capacity.fetch_sub(1, Ordering::Relaxed);
                }
                return output;
            }
        }

        let mut target_table = self.write_state.current_table.load(Ordering::Relaxed);
        loop {
            // SAFETY: the pointers to individual tables are never deallocated until the builder is
            // finished with
            let current_table = unsafe { &*target_table };
            // let remaining_capacity = current_table.remaining_capacity.fetch_sub(1, Ordering::Relaxed);
            // if remaining_capacity >= 0
            // current_table.table.write(hash, value);
            // }
            // let start = SystemTime::now();
            if let Ok(output) = current_table.table.insert(hash, value) {
                // if target_table != self.current_table_cache {
                //     self.writes_to_current_table = 0;
                // }
                self.current_table_cache = target_table;

                // TODO maybe consider using a full flag to indicate that the migration has started
                //  so threads avoid trying to repeatedly write to a full table
                if output == None {
                    // let end = SystemTime::now();
                    // self.time_wasted_due_to_full_table += end.duration_since(start).unwrap().as_nanos() as u64;
                    // self.writes_to_current_table += 1;
                    self.total_writes += 1;
                    // current_table.remaining_capacity.fetch_sub(1, Ordering::Relaxed);
                }

                return output
            }

            self.failed_writes += 1;

            // We use an atomic fence here to allow the initial table load to be relaxed in the
            // happy case; if the table has enough space in it, there is no need to use a stronger
            // ordering than Relaxed. However, if the table is full, we actually need the load to
            // be Acquire, to ensure that we see the Release store in the migration state.
            fence(Ordering::Acquire);

            // Try to create a new table and use that instead. remaining_capacity is negative
            // due to the if condition above.
            target_table = self.create_new_table(current_table, 0)
        }
    }

    fn create_new_table(
        &mut self,
        current_table: &InnerTable<V>,
        overflowed_capacity: usize,
    ) -> *mut InnerTable<V> {
        let existing_capacity = current_table.table.capacity() + current_table.total_capacity_of_previous_tables;
        // Tables grow by two each time
        let desired_capacity = 2 * (existing_capacity + overflowed_capacity);

        let mut previous_in_progress = false;
        let start = SystemTime::now();
        loop {
            let migration_result = self.migrate_table(
                current_table.generation,
                desired_capacity,
                existing_capacity,
            );
            match migration_result {
                Ok(new_table) => {
                    if previous_in_progress {
                        let end = SystemTime::now();
                        self.waiting_for_previous_migration += end.duration_since(start).unwrap().as_nanos() as u64;
                    }
                    return new_table;
                },
                Err(ClaimMigrationFailure::AlreadyInProgress | ClaimMigrationFailure::GenerationOutOfDate) => {
                    // Another thread is already migrating the table, so we need to wait for it to
                    // finish
                    let current_generation = current_table.generation;

                    let start = SystemTime::now();
                    loop {
                        // Need to use Acquire ordering here to ensure that the load of the new table
                        // includes any writes that occurred from other threads
                        let new_table = self.write_state.current_table.load(Ordering::Acquire);
                        let table = unsafe { &*new_table };
                        if table.generation != current_generation {
                            let end = SystemTime::now();
                            self.waiting_for_in_progress_migration += end.duration_since(start).unwrap().as_nanos() as u64;

                            return new_table;
                        }

                        // Spin
                        std::hint::spin_loop();
                    }
                },
                Err(ClaimMigrationFailure::PreviousInProgress) => {
                    // The previous thread hasn't yet finished updating the migration state, so we can
                    // spin while waiting to claim the migration
                    std::hint::spin_loop();
                    previous_in_progress = true;
                },
            }
        }
    }

    fn migrate_table(
        &self,
        current_generation: u32,
        desired_capacity: usize,
        existing_capacity: usize,
    ) -> Result<*mut InnerTable<V>, ClaimMigrationFailure> {
        self.claim_migration_relaxed(current_generation)?;

        let next_generation = current_generation + 1;
        let new_table_box = Box::new(InnerTable::<V>::new(next_generation, desired_capacity, existing_capacity));
        // println!("Created new table with generation {}, capacity {}, from desired capacity {}", next_generation, new_table_box.table.size(), desired_capacity);
        let new_table = Box::into_raw(new_table_box);
        let previous_table = self.write_state.current_table.swap(new_table, Ordering::AcqRel);

        // Now that we have updated the current table, we can mark the migration as successful
        self.complete_migration(current_generation, next_generation);

        // Save the previous table for later migration
        // TODO avoid using a lock here?
        {
            let mut all_tables = self.write_state.full_tables.lock().unwrap();
            // SAFETY: we convert the raw pointer back into a box to make it easier to store and
            // clean up later. Other threads may still have a reference to the pointer, so we ensure
            // that the full_tables vector is not cleaned up until all threads have finished
            // writing, so the pointer will remain valid.
            all_tables.push(unsafe { Box::from_raw(previous_table) });
        }

        Ok(new_table)
    }

    fn claim_migration_relaxed(&self, current_generation: u32) -> Result<(), ClaimMigrationFailure> {
        // Attempt to claim the migration of this generation's table. The migration state contains
        // a generation number in the upper 32 bits, and a flag in the lowest bit which is set to 1
        // when the migration of the current generation has been claimed.
        let current_generation_migration_state = (current_generation as u64) << 32;
        let claimed_migration_state = current_generation_migration_state | 1;
        match self.write_state.migration_state.compare_exchange(
            current_generation_migration_state,
            claimed_migration_state,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            // We are the first thread to try to migrate this table
            Ok(_) => Ok(()),
            Err(new_state) => {
                let migrator_generation = (new_state >> 32) as u32;
                if migrator_generation == current_generation {
                    // Another thread is already migrating the table
                    Err(ClaimMigrationFailure::AlreadyInProgress)
                } else if migrator_generation < current_generation {
                    // The previous migration is not quite complete yet
                    Err(ClaimMigrationFailure::PreviousInProgress)
                } else {
                    // A new table was already created in the meantime
                    Err(ClaimMigrationFailure::GenerationOutOfDate)
                }
            }
        }
    }

    fn complete_migration(&self, current_generation: u32, completed_generation: u32) {
        let migration_in_progress_state = ((current_generation as u64) << 32) | 1;
        let migration_complete_state = (completed_generation as u64) << 32;
        match self.write_state.migration_state.compare_exchange(
            migration_in_progress_state,
            migration_complete_state,
            Ordering::AcqRel,
            Ordering::Relaxed,
        ) {
            Ok(_) => {}
            Err(actual) => {
                panic!("Failed to complete migration, something else must have written to the state in the meantime. Expected state {:0b}, desired state {:0b}, actual state {:0b}", migration_in_progress_state, migration_complete_state, actual);
            }
        }
    }
}

enum ClaimMigrationFailure {
    AlreadyInProgress,
    PreviousInProgress,
    GenerationOutOfDate,
}

#[cfg(test)]
mod tests {
    use std::iter;
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    use crate::new_map_3::new_map_3::WriteOnlyTable;

    #[tokio::test(flavor = "multi_thread")]
    pub async fn make_single_threaded() {
        let mut rng = StdRng::seed_from_u64(123);
        let pairs = (0..10000)
            .map(|_| rng.gen())
            .map(|i| (i, i as usize))
            .collect::<Vec<_>>();
        let mut table = WriteOnlyTable::new();
        for (hash, value) in &pairs {
            assert_eq!(table.insert(*hash, *value), None);
        }

        let read_table = table.compact().await;
        for (hash, value) in &pairs {
            assert_eq!(read_table.get(*hash).copied(), Some(*value), "Expected to find value {} with hash {} in the table", value, hash);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    pub async fn build_table_multi_threaded() {
        let batch_size = 2056;
        let thread_count = 128;
        let mut data = vec![0u64; batch_size * thread_count];
        let mut rng = StdRng::seed_from_u64(123);
        rng.fill(&mut data[..]);

        let pairs: Vec<_> = data.into_iter().enumerate().collect();
        let table = WriteOnlyTable::<usize>::new();

        // Start all the threads in parallel
        let batches: Vec<_> = pairs.chunks(batch_size).collect();
        let handles: Vec<_> = batches.into_iter()
            .zip(iter::repeat(table))
            .map(|(batch, mut table)| {
            let batch = Vec::from(batch);
            tokio::spawn(async move {
                for (value, hash) in batch {
                    table.insert(hash, value);
                }

                table.compact().await
            })
        }).collect();

        let mut tables = vec![];
        for handle in handles {
            tables.push(handle.await.unwrap());
        }

        for table in &tables {
            // Check that all the values are readable in the table
            for (value, hash) in &pairs {
                assert_eq!(table.get(*hash), Some(value), "Expected to find value {} with hash {} in the table", value, hash);
            }
        }
    }
}
