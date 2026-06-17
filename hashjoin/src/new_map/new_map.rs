// use std::collections::HashMap;
// use std::ptr::null;
// use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, AtomicU64, AtomicUsize, Ordering};
// use tokio::sync::Notify;
// // Decrement the remaining space counter
// // If it becomes negative, start the migration process
// // If the counter is exactly -1, we are the first thread responsible for the migration, so we should
// // allocate a new larger map, and store it somewhere.
// // If the counter is < -1, we should participate in the migration. First wait for the new map to be
// // allocated by spinning (?). Then work to migrate each bucket.
// // Once all buckets have been migrated, we need to wait for the migration to complete by spinning
// // again (?).
// // Indicate that the old map is being migrated by incrementing the state counter.
// // Start to migrate each of the old buckets to the new map
//
// struct Bucket {
//     // TODO Use atomic u64
//     state: AtomicUsize,
// }
//
// enum WriteResult {
//     Succeeded,
//     SucceededMustMigrate(usize),
//     FailedSoStartMigration,
//     FailedSoParticipateInMigration,
//     FailedMustMigrate(usize),
// }
//
// struct Table<V> {
//     remaining_capacity_pending: AtomicI64,
//     remaining_capacity_completed: AtomicI64,
//     table: AtomicPtr<InnerTable>,
//     // migrator_ready: Notify,// TODO use safer notify wrapper
//     migrator_state: AtomicU64,
//     migrator_initiator_race: AtomicU64,
//     new_table: AtomicPtr<InnerTable>,
//     next_migrator_bucket: AtomicUsize,
//     completed_migrator_bucket: AtomicUsize,
// }
//
// // Decrement remaining capacity pending
// // If < 0
// // load migrator state by incrementing counter
// // if state == uninitialised, increment race counter to initialise
// // if race won, allocate new table and set new table
// // perform migration
// // thread that finishes the last bucket of the migration should set the state to finalising
//
//
// impl <V> Table<V> {
//     pub fn write(&self, hash: u64, value: V) {
//         self.migrator_initiator_race.fetch_add(1, Ordering::Relaxed);
//         match self.write_inner(hash, value) {
//             WriteResult::Succeeded => {},
//             WriteResult::SucceededMustMigrate(bucket_index) => {
//                 // We successfully wrote the key but must perform the migration of the given bucket.
//             },
//             WriteResult::FailedSoStartMigration => {
//                 // There was not enough room in the table, so we should start the migration
//                 self.initialize_migration();
//                 self.participate_in_migration();
//             },
//             WriteResult::FailedSoParticipateInMigration => {
//                 // There was not enough space, so we should participate in the migration
//             },
//             WriteResult::FailedMustMigrate(bucket_index) => {
//                 // Failed to write the key, because a migration started in the middle of the update.
//             },
//         }
//     }
//
//     pub fn write_inner(&self, hash: u64, value: V) -> WriteResult {
//         // Start by decrementing the remaining space counter to check if there is still space in the
//         // table
//         let remaining = self.remaining_capacity_pending.fetch_sub(1, Ordering::Relaxed);
//         if remaining >= 0 {
//             // There is space in the table, which means we are guaranteed to be able to use the
//             // value in the pointer until we decrement the completed counter
//             let table = self.table.load(Ordering::Relaxed);
//             let table = unsafe { &*table };
//
//             // Write to the table
//             // TODO
//
//             // Once finished, decrement the remaining_space_2 counter
//             self.remaining_capacity_completed.fetch_sub(1, Ordering::Relaxed);
//         } else if remaining == -1 {
//             // We are the first thread to start the migration
//             let new_table = HashMap::with_capacity(128);
//             let migrator = ();
//             self.migrator.store(&migrator, Ordering::Relaxed);
//
//             // Participate in the migration
//         }
//     }
//
//     fn initialize_migration(&self) {
//         // TODO
//         let new_capacity = 128;
//         let migrator = Box::new(Migrator::new(new_capacity));
//         let existing = self.migrator.swap(Box::into_raw(migrator), Ordering::Relaxed);
//         assert!(existing.is_null());
//
//         // TODO notify that migration has started
//     }
//
//     fn participate_in_migration(&self) {
//         // Increment the number of threads using the migrator
//         let new_value = self.migrator_state.fetch_add(1, Ordering::Relaxed);
//         let marked_for_deletion = new_value & 0b10000000_00000000 > 0;
//         if marked_for_deletion {
//             // Migrator is already marked for deletion, so we should not use the pointer anymore.
//             // Attempt to decrement the thread counter
//             let new_value = self.migrator_state.fetch_sub(1, Ordering::Relaxed);
//             let thread_count = new_value & 0b01111111_11111111;
//             if thread_count == 0 {
//                 // If we were the last user of the migrator, there is a chance that we would be
//                 // responsible for deleting it
//                 let new_value = self.migrator_state.fetch_add(0b1_0000000_00000000, Ordering::Relaxed);
//                 let owners_count = new_value >> 16;
//                 if owners_count == 1 {
//                     // We are responsible for deleting the migrator
//                     let migrator = self.migrator.swap(std::ptr::null_mut(), Ordering::Relaxed);
//                     let migrator_ref = unsafe { &mut *migrator };
//                     let new_table = std::mem::replace(&mut migrator_ref.destination, Box::new(InnerTable {
//                         user_count: Default::default(),
//                         buckets: vec![],
//                     }));
//                     // SAFETY: We guaranteed that we are the last user of the migrator pointer
//                     drop(unsafe { Box::from_raw(migrator) });
//
//                     // Now we must wait until all threads have finished using the normal table.
//                     // This is done by spinning on the remaining_capacity_completed counter
//                     while self.remaining_capacity_completed.load(Ordering::Relaxed) > 0 {
//                         // Spin
//                     }
//
//                     // At this point there are no more threads using the source table or the
//                     // migrator, so we can swap them.
//                     let new_table_ptr = Box::into_raw(new_table);
//                     let old_table = self.table.swap(new_table_ptr, Ordering::Relaxed);
//                     // SAFETY: We guaranteed that we are the last user of the table pointer
//                     drop(unsafe { Box::from_raw(old_table) });
//
//                     // The migrator has been reset, the new table is in position. The last thing
//                     // to do is reset all the states ready for the threads to start writing to the
//                     // new table. This needs to be done in the correct order
//                     // TODO use the correct size (total size - previous size)
//                     // TODO use the correct ordering
//                     self.remaining_capacity_completed.store(123, Ordering::Relaxed);
//
//
//                     self.remaining_capacity_pending.store(123, Ordering::Relaxed);
//                 }
//             }
//         }
//
//         let bucket_count = 12;
//         let bucket_counter = AtomicUsize::new(0);
//         let next_bucket = bucket_counter.fetch_add(1, Ordering::Relaxed);
//
//         if next_bucket < bucket_count {
//             // Migrate bucket from old table to new table
//         } else {
//             // We have finished migrating all buckets
//         }
//     }
//
// }
//
// // Loop over each bucket, and attempt to mark that it is going to be migrated.
// fn mark_for_migration(users: &AtomicUsize) {
//     null();
//
//     // Indicate that the migration is starting
//     let new_value = users.fetch_or(0b0_1000000_00000000, Ordering::Relaxed);
//     let writer_count = new_value & 0b01111111_11111111;
//     if writer_count == 0 {
//         // If there were no writers, we are a candidate for being the migrator
//         let new_value = users.fetch_add(0b1_0000000_00000000, Ordering::Relaxed);
//         let migrator_count = new_value >> 16;
//         if migrator_count == 1 {
//             // We are the first migrator, so we win
//         }
//     }
//
//     // // We are the first migrator if the migrator count is exactly 1
//     // let we_are_first_migrator = new_value & 0b11111111_00000000_00000000 == 1;
//     // // There are no other writers to this bucket if the writer count is exactly 0
//     // let we_are_only_writer = new_value & 0b11111111_11111111 == 0;
//
//     // match users.load(Ordering::Acquire) {
//     //     0 => {
//     //         // We are the only user of this bucket
//     //     },
//     //     _ => {
//     //         // There are other users of this bucket
//     //         // One of them will be responsible for migrating the bucket
//     //     }
//     // };
// }
//
// fn user_of_bucket(users: &AtomicUsize) {
//     // Indicate that we are writing to this bucket
//     let new_value = users.fetch_add(1, Ordering::Relaxed);
//
//     // Check that the migration hasn't already started
//     let flagged_for_migration = new_value & 0b10000000_00000000 > 0;
//     if flagged_for_migration {
//         // Another thread already indicated that the migration had started. We should clean up the
//         // writer count
//         // TODO could the sub and later fetch_add be combined with a single fetch_add?
//         //   users.fetch_add(0b1_11111111_11111111, Ordering::Relaxed);
//         let new_value = users.fetch_sub(1, Ordering::Relaxed);
//
//         // If we were the last writer of this bucket, there is a chance that we need to perform the
//         // migration
//         let writer_count = new_value & 0b01111111_11111111;
//         if writer_count == 0 {
//             // Increment the migrator count
//             let new_value = users.fetch_add(0b1_0000000_00000000, Ordering::Relaxed);
//             let migrator_count = new_value >> 16;
//             if migrator_count == 1 {
//                 // We are the migrator for this bucket
//             }
//         }
//     }
//
//     // // Update the values of the bucket
//     //
//     // // Indicate that we are done writing to this bucket
//     // users.fetch_sub(1, Ordering::Relaxed);
//     //
//     // // Check that the migration hasn't already started
//     // if !bucket_state.load(Ordering::Acquire) {
//     //
//     //     // Perform some write
//     //
//     //     // Indicate that we are done writing to this bucket
//     //     users.fetch_sub(1, Ordering::Release);
//     // }
// }
//
// struct InnerTable {
//     user_count: AtomicUsize,
//     buckets: Vec<Bucket>,
// }
//
// impl InnerTable {
//     fn bucket_count(&self) -> usize {
//
//     }
// }
//
// struct Migrator<V> {
//     next_bucket_counter: AtomicUsize,
//     completed_bucket_counter: AtomicUsize,
//     destination: Box<InnerTable>,
// }
//
// impl <V> Migrator<V> {
//     fn new(num_buckets: usize) -> Self {
//         let destination = Box::new(InnerTable {
//             user_count: AtomicUsize::new(0),
//             buckets: Vec::with_capacity(num_buckets),
//         });
//         Self {
//             next_bucket_counter: AtomicUsize::new(0),
//             completed_bucket_counter: AtomicUsize::new(0),
//             destination,
//         }
//     }
//
//     fn participate_in_migration(&self, source_table: &InnerTable) {
//         loop {
//             let next_bucket = self.next_bucket_counter.fetch_add(1, Ordering::Relaxed);
//
//             if next_bucket < source_table.bucket_count() {
//                 // Migrate bucket from old table to new table
//
//                 // Mark bucket as being migrated, and if we are the migrator of this bucket, move
//                 // the values into the destination table.
//             } else {
//                 // We have finished migrating all buckets
//
//             }
//         }
//     }
// }
//
// struct MapInner {
//     // Stores whether
//     state: AtomicU64,
//     // map:
// }
//
// #[cfg(test)]
// mod tests {
//     #[test]
//     fn print() {
//         let value = 0b101;
//         println!("{:#034b}", value);
//         let value = value + 0b1_11111111;
//         println!("{:#034b}", value);
//         let value = value + 1;
//         let value = value + 0b1_11111111;
//         println!("{:#034b}", value);
//     }
// }
