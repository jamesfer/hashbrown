# New map 2 improvements

- Include an option to write multiple entries in bulk, or lookup multiple entries in bulk
- Avoid having to search the entire table for empty spots before discovering it is full
- Try to reduce the amount of time multiple threads are waiting for the next generation












----------------------

1. Build bucketed btrees locally. Number of buckets should equal number of threads
2. Each thread merges its own bucketed btree.
3. Each thread loops over bucketed trees in order and counts number of entries in each destination
   bucket.

We only need 
- a way to partition the writes to the destination to prevent conflicts.
- a way to write keys consistently to the destination
- an easy way to deduplicate entries during merging

loop over entries in source map. determine destination spot, check if empty. 
  if spot is available, set occupied flag.
    if we own the destination spot, write to hash map.
  if occupied, probe to next spot
  if occupied and value is the same, handle duplicate
// Every thread would need to compute the duplicates of every entry


sort each incoming vec of hashes using a vec of indices
  // their initial indices will need to be tracked because that value is used by the join operator
  // input here has 0 nulls, Vec<u64>
loop the sorted input and write duplicates to the overflow buffers
  // how are duplicates removed from the array?
    // Record duplicates in secondary array (boolean bit map or list of indices?)
    // The next time the vector is copied into another destination, skip the duplicates
increment a global counter to track total size // Maybe each thread increments an independent counter
allocate a destination buffer with correct size
write the single sorted input buffer from each thread to the destination buffer (skipping the duplicates found locally)
  // Can be done in parallel without synchronisation since every thread knows the size of everyone
  // else's buffers
collaboratively merge sort the entire buffer, duplicates will be left in the array
  // --Each part of the overall dataset will be handled by a unique thread so that thread can resolve
  // --duplicates by writing directly to the overflow.
  // Instead, duplicates could be left in the array, and the thread writing the results to the final
  // map will be responsible for handling them
allocate an "occupied" vec of N booleans in each thread
iterate over the merged buffer
  if duplicate:
    if owned, write to global overflow buffers
    if not skip
  perform probing until an empty spot in the occupied vec is found
  if the entry is owned by the thread, write to it in the final map.

In this approach, duplicates within each thread are resolved each time two vectors are merged
Then when the locally accumulated vectors are sorted in the final destination, duplicates are kept,
but written to the overflow during iteration. At this point, there could only be a maximum of T
duplicates for each value where T is the number of threads.


// Doesn't work
build a partitioned hash map locally in each thread
once finished
increment a global counter to track the total size
add each partitioned hash map to a global channel
each thread iterates over the global counter






10111010010101
