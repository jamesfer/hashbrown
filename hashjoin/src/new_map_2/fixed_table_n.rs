use crate::new_map_2::atomic::{AsAtomic, AtomicOps};
use crate::new_map_2::chunk::Chunk;
use crate::new_map_2::chunk_8::Chunk8;
use crate::new_map_2::iterable_bit_mask::IterableBitMaskT;
use crate::new_map_2::iterable_bit_mask_8::{IterableBitMask8, IterableBitMaskIntrinsics8};
use std::cell::UnsafeCell;
use std::cmp::max;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

// Highest bit
const OVERFLOW_BIT_MASK: u64 = 1 << 63;
const IGNORE_TOP_7_MASK: u64 = 0x1FFF_FFFF_FFFF_FFFF;

// The top 7 bits of the hash are ignored when stored in the table, as those bits are already stored
// in the tag.
const STORED_HASH_BIT_MASK: u64 = 0x1FFF_FFFF_FFFF_FFFF;
// 6th-highest bit, the lowest unused bit in the stored hash
const HASH_OCCUPIED_BIT: u64 = 0x4000_0000_0000_0000;

const STORED_OCCUPIED_HASH_BIT_MASK: u64 = STORED_HASH_BIT_MASK | HASH_OCCUPIED_BIT;

// const POSITIONAL_OVERFLOW_BIT_OR_MASKS: [u64; 16] = [
//     calculate_positional_overflow_bit_or_mask(0),
//     calculate_positional_overflow_bit_or_mask(1),
//     calculate_positional_overflow_bit_or_mask(2),
//     calculate_positional_overflow_bit_or_mask(3),
//     calculate_positional_overflow_bit_or_mask(4),
//     calculate_positional_overflow_bit_or_mask(5),
//     calculate_positional_overflow_bit_or_mask(6),
//     calculate_positional_overflow_bit_or_mask(7),
//     calculate_positional_overflow_bit_or_mask(8),
//     calculate_positional_overflow_bit_or_mask(9),
//     calculate_positional_overflow_bit_or_mask(10),
//     calculate_positional_overflow_bit_or_mask(11),
//     calculate_positional_overflow_bit_or_mask(12),
//     calculate_positional_overflow_bit_or_mask(13),
//     calculate_positional_overflow_bit_or_mask(14),
//     calculate_positional_overflow_bit_or_mask(15),
// ];
//
// // Position can only be a number between 0 and 15
// // Produces a mask that is all zeros, except the most significant bit which is only set if position
// // is equal to 15
// const fn calculate_positional_overflow_bit_or_mask(position: usize) -> u64 {
//     // ((position as u64) + ((1u64 << 63) - 15)) & 1u64 << 63
//     // (position as u64).wrapping_sub(14) << 63
//     ((position as u64 + 1) >> 3) << 63
//     // ((position as u64) + LESS_15) & OVERFLOW_BIT_MASK
// }

// Position can only be a number between 0 and 15
// Produces a mask that is all zeros, except the most significant bit which is only set if position
// is equal to 15
const fn get_positional_overflow_bit_or_mask(position: usize) -> u64 {
    // ((position as u64) + ((1u64 << 63) - 15)) & 1u64 << 63
    // (position as u64).wrapping_sub(14) << 63
    ((position as u64 + 1) >> 4) << 63
    // ((position as u64) + LESS_15) & OVERFLOW_BIT_MASK
    // POSITIONAL_OVERFLOW_BIT_OR_MASKS[position & (16 - 1)]
}


fn overwrite_overflow_bit(hash: u64, tag: u8) -> u64 {
    (hash & !OVERFLOW_BIT_MASK) | (((tag & 0b1000_0000) as u64) << 56)
}

#[cfg(test)]
mod utils_tests {
    use crate::new_map_2::fixed_table_n::{get_positional_overflow_bit_or_mask, overwrite_overflow_bit, OVERFLOW_BIT_MASK};

    #[test]
    pub fn get_positional_overflow_bit_or_mask_works() {
        for i in 0..=14 {
            assert_eq!(get_positional_overflow_bit_or_mask(i), 0);
        }
        assert_eq!(get_positional_overflow_bit_or_mask(15), 1 << 63);
    }

    #[test]
    pub fn overwrite_overflow_bit_works() {
        assert_eq!(overwrite_overflow_bit(0, 0), 0);
        assert_eq!(overwrite_overflow_bit(123, 0), 123);
        assert_eq!(overwrite_overflow_bit(123 & OVERFLOW_BIT_MASK, 0), 0);
        assert_eq!(overwrite_overflow_bit(123 & OVERFLOW_BIT_MASK, 0b1000_0000), 0 | OVERFLOW_BIT_MASK);
        assert_eq!(overwrite_overflow_bit(123, 0b1000_0000), 123 | OVERFLOW_BIT_MASK);
    }
}


pub struct FixedTable8<V> {
    size: usize,
    chunks: UnsafeCell<Vec<Chunk8<V>>>,
}

unsafe impl <T: Send> Send for FixedTable8<T> {}
unsafe impl <T: Sync> Sync for FixedTable8<T> {}

impl <V> FixedTable8<V>
where V: Default + Copy + AsAtomic + PartialEq + 'static
{
    pub fn new_with_capacity(capacity: usize) -> Self {
        let chunk_count = (capacity.next_multiple_of(8) / 8).next_power_of_two();
        Self {
            size: chunk_count * 8,
            chunks: UnsafeCell::new((0..chunk_count).map(|_| Chunk8::new()).collect()),
        }
    }

    pub fn new(chunk_count: usize) -> Self {
        Self {
            size: chunk_count * 8,
            chunks: UnsafeCell::new((0..chunk_count).map(|_| Chunk8::new()).collect()),
        }
    }

    // TODO Rename to capacity
    pub fn size(&self) -> usize {
        self.size
    }

    pub fn to_read_only(self) -> ReadOnlyTable8<V> {
        ReadOnlyTable8::new(self.chunks)
    }

    // TODO This method is only safe during compaction. How should we protect against that?
    pub fn entries(&self) -> impl Iterator<Item=(u64, V)> {
        unsafe { &*self.chunks.get() }.iter()
            .map(|chunk| {
                // For the last entry in each chunk, we want to use the highest bit from the tag
                // instead of the hash, since one of the bits in the hash is used for the overflow
                // bit.
                // TODO use vectorised instructions here?
                chunk.tags.iter()
                    .zip(chunk.data[0..7].iter())
                    .filter(|(_, (hash, _))| *hash & HASH_OCCUPIED_BIT != 0)
                    .map(|(tag, (hash, value))| ((*hash & STORED_HASH_BIT_MASK) | ((*tag as u64) << 56), value.clone()))
            })
            .flatten()
    }

    pub fn write(&self, hash: u64, value: V) -> Result<Option<V>, ()> {
        let mut loaded_tags = [0u8; 8];
        let mut match_output = [0u8; 8];
        self.write_using_buffers(hash, value, &mut loaded_tags, &mut match_output)
    }

    // TODO remove this method as it doesn't seem to be efficient
    pub fn write_using_buffers(
        &self,
        hash: u64,
        value: V,
        loaded_tags: &mut [u8; 8],
        match_output: &mut [u8; 8],
    ) -> Result<Option<V>, ()> {
        // TODO add safety comment
        let chunks = unsafe { &mut *self.chunks.get() };

        // Compute starting position of probe sequence
        // TODO extract to function
        let top_8_bits = (hash >> 56) as u8;
        let search_tag = max(top_8_bits, 1);
        let stored_hash = hash & STORED_HASH_BIT_MASK | HASH_OCCUPIED_BIT;
        let stride_width = (top_8_bits as usize) * 2 + 1;
        let mut index = hash as usize;
        let mut attempts = 0;

        // let mut history = Vec::with_capacity(chunks.len());
        while attempts < chunks.len() {
            // Calculate the chunk index
            let chunk_index = index % chunks.len(); // TODO use & instead
            let chunk = &mut chunks[chunk_index];

            // Load tags value. Check for existing matches
            //   Load corresponding values and compare.
            //   Might need to use a fence here to ensure that the value load happens after the tag
            //   write.
            // No matches
            // Search the tags value for empty slots
            // Loop over each empty spot and CAS the full hash into the values list.
            //   If CAS fails and the existing hash matches our one, then we have a duplicate key.
            //     Execute a swap on the value to extract the old one and insert the new one.
            //     This would mean we only support atomic values.
            //     The swap could also produce an empty value if the other thread's write hasn't
            //     yet completed
            //   If CAS fails with a random hash, cell was already occupied, continue to next
            //   If CAS succeeds, we have inserted the value, so update the tag value with our tag.
            //     This could be done with a non-atomic write as the fact we won the race to write
            //     the full hash means we are the only writer to that exact tag.

            // Load the tags value
            for (tag, output) in chunk.tags.iter_mut().zip(loaded_tags.iter_mut()) {
                let tag_atomic = unsafe { Self::atomic_tag_ref(tag) };
                *output = tag_atomic.load(Ordering::Relaxed);
            }

            // history.push((chunk_index, loaded_tags.clone()));

            // Check if the value already appears in this cell
            // let overflow_bit = loaded_tags[15] & 0b1000_0000; // TODO Extract constant mask

            // search_tag_buffer.fill(search_tag);
            // The highest bit of the last tag in each chunk is a special flag that is only set to 1
            // once the chunk overflows, so the last entry should have its final bit set to the
            // same value as the flag.
            // search_tag_buffer[15] = (search_tag_buffer[15] & 0b0111_1111) | overflow_bit;

            // Check if the search tag appears in the chunk's tags. This uses a simple loop that
            // makes use of LLVM's auto-vectorization capabilities
            // TODO replace with intrinsics
            let iter = loaded_tags.iter()
                // .zip(search_tag_buffer.iter())
                .zip(match_output.iter_mut());
            for (tag, output) in iter {
                *output = u8::from(*tag == search_tag);
            }

            let mut bit_mask = IterableBitMask8::new(match_output.clone());
            while let Some(position) = bit_mask.next_set_index() {
                let item = &mut chunk.data[position];
                let atomic_hash = unsafe { Self::atomic_hash_ref(item) };
                let item_hash = atomic_hash.load(Ordering::Relaxed);

                let masked_item_hash = item_hash & STORED_OCCUPIED_HASH_BIT_MASK;
                if masked_item_hash == stored_hash {
                    // Found a match
                    let atomic_value = unsafe { Self::atomic_value_ref(item) };
                    let previous = atomic_value.swap(value, Ordering::Relaxed);

                    // println!("Overwrote value {:?} with {:?} at position {} in chunk {}, masked hash {:0b}", previous, value, position, chunk_index, masked_item_hash);

                    // Due to unpredictable orderings between atomic write, it is possible that the
                    // hash was set but no the tag, so we still need to check if the value is 0
                    return Ok(Self::default_to_option(previous));
                } else if masked_item_hash == 0 {
                    // Due to the unpredictable ordering of atomic writes, it is possible that this
                    // thread saw the updated tag value but not the updated hash value. In which
                    // case we can just mark the tag value as empty so we re-check it again later.
                    loaded_tags[position] = 0;
                }
            }

            // No matches

            // Search for empty slots
            // TODO replace with intrinsics
            let iter = loaded_tags.iter().zip(match_output.iter_mut());
            for (tag, output) in iter {
                *output = u8::from(*tag == 0);
            }
            // TODO remove the two clones
            let mut bit_mask = IterableBitMask8::new(match_output.clone());
            while let Some(position) = bit_mask.next_set_index() {
                // Try to write the value into the empty slot
                let data = &mut chunk.data[position];
                let atomic_hash = unsafe { Self::atomic_hash_ref(data) };

                // The overflow bit mask is all ones unless we are in the last position of a chunk
                // let overflow_bit_or_mask = get_positional_overflow_bit_or_mask(position);
                let masked_stored_hash = stored_hash;

                let previous = match atomic_hash.compare_exchange(0, masked_stored_hash, Ordering::Relaxed, Ordering::Relaxed) {
                    Ok(_) => {
                        // We write to the value with a swap operation because there is a chance
                        // that another thread tried to write to the value at the same time
                        let atomic_value = unsafe { Self::atomic_value_ref(data) };
                        let previous = atomic_value.swap(value, Ordering::Relaxed);

                        // Write to the tag to claim it as well
                        let atomic_tag = unsafe { Self::atomic_tag_ref(&mut chunk.tags[position]) };
                        atomic_tag.store(search_tag, Ordering::Relaxed);

                        // println!("Just stored {} with value {:?} and tag {} at position {} in chunk {}, (masked hash {}, with bit {})", hash, value, search_tag, position, chunk_index, masked_stored_hash, masked_stored_hash | OVERFLOW_BIT_MASK);

                        previous
                    },
                    Err(previous) if previous == masked_stored_hash => {
                        // Detected duplicate late. Swap the existing value with our one
                        let atomic_value = unsafe { Self::atomic_value_ref(data) };
                        let previous = atomic_value.swap(value, Ordering::Relaxed);
                        previous
                    },
                    Err(previous) if previous == get_positional_overflow_bit_or_mask(position) => {
                        // panic if the overflow bit was already set without a value being written.
                        // It is possible that this is a valid but uncommon state due to the
                        // unpredictable ordering of atomic values, but for now we treat it as if
                        // it should never happen.
                        panic!("Overflow bit was set without a value being written. Hash {}, value {:?}, attempts {}, chunks {}", hash, value, attempts, chunks.len());
                    },
                    // The cell was written to by another thread after we loaded the tag values
                    Err(_) => continue,
                };

                // We successfully wrote the value
                return Ok(Self::default_to_option(previous));
            }

            // Chunk is full, probe to the next one
            index = index.wrapping_add(stride_width);
            attempts += 1;

            // If we caused an overflow of this chunk, we need to set the overflow bit.
            let data = &mut chunk.data[7];
            let atomic_hash = unsafe { Self::atomic_hash_ref(data) };
            let overflow_bit = atomic_hash.load(Ordering::Relaxed) & OVERFLOW_BIT_MASK;
            if overflow_bit == 0 {
                atomic_hash.fetch_or(OVERFLOW_BIT_MASK, Ordering::Relaxed);
            }
        }

        // There was not enough space in the table to store a new value
        // panic!("Table is full. Hash {hash}, value {:?}, attempts {attempts}, chunks {}, history {:?}", value, chunks.len(), history);
        Err(())
    }

    fn default_to_option(value: V) -> Option<V> {
        if value == V::default() {
            None
        } else {
            Some(value)
        }
    }

    #[inline(always)]
    unsafe fn atomic_hash_ref(item: &mut (u64, V)) -> &AtomicU64 {
        let hash_ref = &mut item.0;
        let hash_ptr = core::ptr::from_mut(hash_ref);
        AtomicU64::from_ptr(hash_ptr)
    }

    #[inline(always)]
    unsafe fn atomic_value_ref(item: &mut (u64, V)) -> &V::AtomicT {
        let value_ref = &mut item.1;
        let value_ptr = core::ptr::from_mut(value_ref);
        V::AtomicT::from_ptr(value_ptr)
    }

    #[inline(always)]
    unsafe fn atomic_tag_ref(tag_ref: &mut u8) -> &AtomicU8 {
        let tag_ptr = core::ptr::from_mut(tag_ref);
        AtomicU8::from_ptr(tag_ptr)
    }
}

pub struct ReadOnlyTable8<V> {
    chunk_mask: usize,
    chunks: UnsafeCell<Vec<Chunk8<V>>>
}

unsafe impl <T: Send> Send for ReadOnlyTable8<T> {}
unsafe impl <T: Sync> Sync for ReadOnlyTable8<T> {}

impl <V> ReadOnlyTable8<V> {
    fn new(chunks: UnsafeCell<Vec<Chunk8<V>>>) -> Self {
        let chunk_mask = {
            let chunks = unsafe { &*chunks.get() };
            chunks.len() - 1
        };
        Self { chunk_mask, chunks }
    }

    pub fn get(&self, hash: u64) -> Option<&V> {
        let chunks = unsafe { &*self.chunks.get() };

        // The tag 0 is reserved to represent an empty cell
        let top_8_bits = (hash >> 56) as u8;
        let search_tag = max(top_8_bits, 1);
        let stored_hash = hash & STORED_HASH_BIT_MASK | HASH_OCCUPIED_BIT;
        let stride_width = (top_8_bits as usize) * 2 + 1;
        let mut index = (hash as usize) & self.chunk_mask;
        let mut attempts = 0usize;

        // println!("looking for hash {:016x} (stored hash {:016x})", hash, stored_hash);

        loop {
            let chunk_index = index;
            let chunk = &chunks[chunk_index];
            // let overflow_bit = chunk.tags[15] & 0b1000_0000; // TODO Extract constant mask

            // search_tag_buffer.fill(search_tag);
            // The highest bit of the last tag in each chunk is a special flag that is only set to 1
            // once the chunk overflows, so the last entry should have its final bit set to the
            // same value as the flag.
            // search_tag_buffer[15] = (search_tag_buffer[15] & 0b0111_1111) | overflow_bit;

            let mut bit_mask = Self::iter_matched_tags(search_tag, chunk);
            while let Some(position) = bit_mask.next_set_index() {
                let item = &chunk.data[position];
                // let overflow_bit_or_mask = get_positional_overflow_bit_or_mask(position);
                // if (item.0 | overflow_bit_or_mask) == (stored_hash | overflow_bit_or_mask) {

                // println!("Checking position {} in chunk {}, item's hash {:016x}, our hash {:016x}", position, chunk_index, item.0, stored_hash);

                if item.0 & STORED_OCCUPIED_HASH_BIT_MASK == stored_hash {
                    return Some(&item.1);
                }
            }

            // If the overflow bit isn't set we can short circuit out of the search loop early
            let overflow_bit = chunk.data[7].0 & OVERFLOW_BIT_MASK;
            if overflow_bit == 0 {
                return None;
            }

            index = (index + stride_width) & self.chunk_mask;
            attempts += 1;
            if attempts >= chunks.len() {
                break;
            }
        }

        None
    }

    pub fn measure_get_stats(&self, hash: u64) -> (Option<&V>, ReadStats) {
        let chunks = unsafe { &*self.chunks.get() };

        // The tag 0 is reserved to represent an empty cell
        let top_8_bits = (hash >> 56) as u8;
        let search_tag = max(top_8_bits, 1);
        let stored_hash = hash & STORED_HASH_BIT_MASK | HASH_OCCUPIED_BIT;
        let stride_width = (top_8_bits as usize).wrapping_mul(2).wrapping_add(1);
        let mut index = hash as usize & self.chunk_mask;
        let mut attempts = 0;
        let mut tag_false_positives = 0;

        // println!("looking for hash {:016x} (stored hash {:016x})", hash, stored_hash);

        while attempts < chunks.len() {
            let chunk_index = index;
            let chunk = &chunks[chunk_index];
            // let overflow_bit = chunk.tags[15] & 0b1000_0000; // TODO Extract constant mask

            // search_tag_buffer.fill(search_tag);
            // The highest bit of the last tag in each chunk is a special flag that is only set to 1
            // once the chunk overflows, so the last entry should have its final bit set to the
            // same value as the flag.
            // search_tag_buffer[15] = (search_tag_buffer[15] & 0b0111_1111) | overflow_bit;

            let mut bit_mask = Self::iter_matched_tags(search_tag, chunk);
            while let Some(position) = bit_mask.next_set_index() {
                let item = &chunk.data[position];
                // let overflow_bit_or_mask = get_positional_overflow_bit_or_mask(position);
                // if (item.0 | overflow_bit_or_mask) == (stored_hash | overflow_bit_or_mask) {

                // println!("Checking position {} in chunk {}, item's hash {:016x}, our hash {:016x}", position, chunk_index, item.0, stored_hash);

                if item.0 & STORED_OCCUPIED_HASH_BIT_MASK == stored_hash {
                    return (Some(&item.1), ReadStats { chunks_accessed: attempts + 1, tag_false_positives});
                } else {
                    tag_false_positives += 1;
                }
            }

            // If the overflow bit isn't set we can short circuit out of the search loop early
            let overflow_bit = chunk.data[7].0 & OVERFLOW_BIT_MASK;
            if overflow_bit == 0 {
                return (None, ReadStats { chunks_accessed: attempts + 1, tag_false_positives });
            }

            index = (index + stride_width) & self.chunk_mask;
            attempts += 1;
        }

        (None, ReadStats { chunks_accessed: attempts + 1, tag_false_positives })
    }

    fn iter_matched_tags(search_tag: u8, chunk: &Chunk8<V>) -> impl IterableBitMaskT {
        #[cfg(target_arch = "aarch64")]
        {
            use crate::new_map_2::intrinsics::u8x8_compare_eq_aarch64;

            let value = u8x8_compare_eq_aarch64(search_tag, &chunk.tags);
            IterableBitMaskIntrinsics8::new(value)
        }

        #[cfg(not(target_arch = "aarch64"))]
        {
            // Fallback
            let mut output = [0u8; 16];
            let iter = chunk.tags.iter().zip(match_output.iter_mut());
            for (tag, output) in iter {
                *output = u8::from(*tag == search_tag);
            }

            IterableBitMask::new(match_output)
        }
    }
}

pub struct ReadStats {
    pub chunks_accessed: usize,
    pub tag_false_positives: usize,
}

#[cfg(test)]
mod tests {
    use crate::new_map_2::fixed_table_n::FixedTable8;
    use rand::rngs::StdRng;
    use rand::seq::SliceRandom;
    use rand::{Rng, SeedableRng};
    use std::collections::HashMap;
    use std::sync::Arc;

    #[test]
    fn create_empty_table() {
        let table = FixedTable8::<usize>::new_with_capacity(20);
        assert_eq!(table.size, 32);

        let read_table = table.to_read_only();
        assert_eq!(read_table.get(0), None);
        assert_eq!(read_table.get(123), None);
        assert_eq!(read_table.get(u64::MAX), None);
    }

    #[test]
    fn can_read_values() {
        let table = FixedTable8::<usize>::new_with_capacity(20);
        table.write(0, 10).unwrap();
        table.write(123, 1230).unwrap();
        table.write(u64::MAX, usize::MAX).unwrap();

        let read_table = table.to_read_only();
        assert_eq!(read_table.get(0).copied(), Some(10));
        assert_eq!(read_table.get(123).copied(), Some(1230));
        assert_eq!(read_table.get(u64::MAX).copied(), Some(usize::MAX));

        assert_eq!(read_table.get(456), None);
        assert_eq!(read_table.get(u64::MAX - 1), None);
    }

    #[test]
    fn can_read_overflowed_values() {
        let table = FixedTable8::<usize>::new_with_capacity(20);
        // Store 17 numbers which all belong to the same initial chunk
        for i in 0..17u64 {
            table.write(i << 32, i as usize).unwrap();
        }

        let read_table = table.to_read_only();
        for i in 0..17u64 {
            assert_eq!(read_table.get(i << 32).copied(), Some(i as usize));
        }
    }

    #[test]
    fn can_read_from_a_full_table() {
        let table = FixedTable8::<usize>::new_with_capacity(64);
        let mut hashes = [0u64; 64];
        let mut rng = StdRng::seed_from_u64(123);
        rng.fill(&mut hashes[..]);

        for hash in hashes {
            table.write(hash, hash as usize).unwrap();
        }

        let read_table = table.to_read_only();
        for hash in hashes {
            assert_eq!(read_table.get(hash).copied(), Some(hash as usize));
        }
    }

    #[test]
    fn write_returns_err_when_table_is_full() {
        let table = FixedTable8::<usize>::new_with_capacity(64);
        let mut hashes = [0u64; 64];
        let mut rng = StdRng::seed_from_u64(123);
        rng.fill(&mut hashes[..]);

        for hash in hashes {
            table.write(hash, hash as usize).unwrap();
        }

        assert_eq!(table.write(1, 1), Err(()));
    }

    #[test]
    fn can_store_all_special_values() {
        let pairs: Vec<_> = (56..64).map(|shift| 1u64 << shift).enumerate().collect();

        // Check that each of the special tag values can be stored in the same chunk without
        // colliding
        let table = FixedTable8::<usize>::new_with_capacity(16);
        for (value, hash) in pairs.iter() {
            table.write(*hash, *value).unwrap();
        }

        let read_table = table.to_read_only();
        for (value, hash) in pairs.iter() {
            assert_eq!(read_table.get(*hash), Some(value));
        }
    }

    #[test]
    fn can_store_zero_hash() {
        let table = FixedTable8::<usize>::new_with_capacity(64);
        assert_eq!(table.write(0, 100), Ok(None));
        for i in 1..64 {
            assert_eq!(table.write(i, i as usize), Ok(None));
        }

        let read_table = table.to_read_only();
        assert_eq!(read_table.get(0).copied(), Some(100));
    }

    #[test]
    fn can_detect_duplicates() {
        let table = FixedTable8::<usize>::new_with_capacity(64);
        assert_eq!(table.write(1, 1), Ok(None));
        assert_eq!(table.write(1, 2), Ok(Some(1)));
        assert_eq!(table.write(4023, 4), Ok(None));
        assert_eq!(table.write(1, 5), Ok(Some(2)));
        assert_eq!(table.write(4023, 6), Ok(Some(4)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn multithreaded_writing_test() {
        let batch_size = 8192;
        let thread_count = 128;
        let mut data = vec![0u64; batch_size * thread_count];
        let mut rng = StdRng::seed_from_u64(123);
        rng.fill(&mut data[..]);

        let pairs: Vec<_> = data.into_iter().enumerate().collect();
        let table = Arc::new(FixedTable8::<usize>::new_with_capacity(batch_size * thread_count));

        // Start all the threads in parallel
        let batches: Vec<_> = pairs.chunks(batch_size).collect();
        let handles: Vec<_> = batches.into_iter().map(|batch| {
            let table = Arc::clone(&table);
            let batch = Vec::from(batch);
            tokio::spawn(async move {
                for (i, hash) in batch {
                    table.write(hash, i).unwrap();
                }
            })
        }).collect();

        for handle in handles {
            handle.await.unwrap();
        }

        // Check that all the values are readable in the table
        let read_table = Arc::into_inner(table).unwrap().to_read_only();
        for (i, hash) in pairs {
            assert_eq!(read_table.get(hash).copied(), Some(i));
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn multithreaded_writing_with_duplicates() {
        let duplicates_count = 8;
        let batch_size = 10;
        let thread_count = 128;
        let mut data = vec![0u64; batch_size * thread_count];
        let mut rng = StdRng::seed_from_u64(456);
        rng.fill(&mut data[..]);

        let mut pairs: Vec<_> = (1usize..duplicates_count + 1)
            .map(|v| data.iter().map(|hash| (*hash, v)).collect::<Vec<_>>())
            .flatten()
            .collect();
        pairs.shuffle(&mut rng);
        let table = Arc::new(FixedTable8::<usize>::new_with_capacity(batch_size * thread_count));

        // Start all the threads in parallel
        // let all_duplicates: Arc<Mutex<HashMap<u64, Vec<usize>>>> = Arc::new(Mutex::new(HashMap::new()));
        let batches: Vec<_> = pairs.chunks(batch_size).collect();
        let handles: Vec<_> = batches.into_iter().map(|batch| {
            let table = Arc::clone(&table);
            let batch = Vec::from(batch);
            tokio::spawn(async move {
                let mut local_duplicates: HashMap<u64, Vec<usize>> = HashMap::new();

                for (hash, v) in batch {
                    match table.write(hash, v).unwrap() {
                        None => {},
                        Some(previous) => {
                            local_duplicates.entry(hash)
                                .and_modify(|vec| vec.push(previous))
                                .or_insert_with(|| vec![previous]);
                        },
                    }
                }

                local_duplicates

                // // Copy local duplicates to global duplicates
                // let all_duplicates = all_duplicates.lock().await.(local_duplicates);
            })
        }).collect();

        let mut outputs = Vec::new();
        for handle in handles {
            outputs.push(handle.await.unwrap());
        }

        // Check that all the values are readable in the table and contain all the correct values
        let read_table = Arc::into_inner(table).unwrap().to_read_only();
        let mut values = Vec::with_capacity(duplicates_count);
        for hash in data {
            values.clear();

            let final_value = read_table.get(hash).copied();
            assert!(final_value.is_some());
            values.push(final_value.unwrap());

            for output in &outputs {
                if let Some(duplicates) = output.get(&hash) {
                    values.extend_from_slice(duplicates);
                }
            }

            values.sort_unstable();
            assert_eq!(values, (1usize..duplicates_count + 1).collect::<Vec<_>>());
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn supports_writing_zero_hash_in_parallel() {
        let mut rng = StdRng::seed_from_u64(123);
        let table_count = 10usize;
        let thread_count = 16usize;

        // For each table create a shuffled vec of the numbers from 0 to 16, excluding 1 since it
        // is handled the same as the hash 0
        let values: Arc<Vec<_>> = Arc::new((0..table_count).map(|_| {
            let mut block = (1..thread_count as u64 + 1).into_iter().collect::<Vec<_>>();
            block[0] = 0;
            block.shuffle(&mut rng);
            block
        }).collect());

        // Run the test in a single thread, which should always succeed
        let tables = (0..table_count).map(|_| FixedTable8::<usize>::new_with_capacity(thread_count)).collect::<Vec<_>>();
        for thread_index in 0..thread_count {
            for ((table_index, table), values_block) in tables.iter().enumerate().zip(values.iter()) {
                let hash = values_block[thread_index];
                table.write(hash, 100 + thread_index).unwrap();
            }
        }

        // Assert that every table has the correct values
        for (table_index, (table, values_block)) in tables.into_iter().zip(values.iter()).enumerate() {
            let read_table = table.to_read_only();
            for (index, hash) in values_block.iter().enumerate() {
                assert_eq!(
                    read_table.get(*hash).copied(), Some(100 + index),
                    "Attempting to read hash {} from table number {}",
                    hash,
                    table_index
                );
            }
        }

        // Then run the test again in parallel, which should fail
        let tables = Arc::new((0..table_count).map(|_| FixedTable8::<usize>::new_with_capacity(thread_count)).collect::<Vec<_>>());
        let barrier = Arc::new(tokio::sync::Barrier::new(thread_count));
        let handles: Vec<_> = (0..thread_count).map(|thread_index| {
            let barrier = Arc::clone(&barrier);
            let tables = Arc::clone(&tables);
            let values = Arc::clone(&values);
            tokio::spawn(async move {
                for ((table_index, table), values_block) in tables.iter().enumerate().zip(values.iter()) {
                    let hash = values_block[thread_index];
                    barrier.wait().await;
                    table.write(hash, 100 + thread_index).unwrap();
                }
            })
        }).collect();

        for handle in handles {
            handle.await.unwrap();
        }

        // Assert that every table has the correct values
        for (table_index, (table, values_block)) in Arc::into_inner(tables).unwrap().into_iter().zip(values.iter()).enumerate() {
            let read_table = table.to_read_only();
            for (index, hash) in values_block.iter().enumerate() {
                assert_eq!(
                    read_table.get(*hash).copied(), Some(100 + index),
                    "Attempting to read hash {} from table number {}",
                    hash,
                    table_index
                );
            }
        }
    }
}
