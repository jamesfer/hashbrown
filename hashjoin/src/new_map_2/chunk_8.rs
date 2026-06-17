pub struct Chunk8<V> {
    // Tags must be kept in a contiguous block of memory to allow for vectorised searching
    pub tags: [u8; 8],
    // Whether it is better to store the hashes and values are stored interleaved or in two separate
    // arrays probably depends on the size of the values. If the hash and value fit on the same
    // cacheline, then reading the value after passing the hash check should be very quick. If the
    // values are large (maybe larger than 64 bits) then it is probably better to store them
    // separately, however, this is just a guess.
    // I believe Rust's default implementation stores hashes and values together. Since this
    // hash map is most commonly going to be storing integers, I just picked to store them together.
    pub data: [(u64, V); 8],
}

impl <V> Chunk8<V>
where V: Default + Copy
{
    pub fn new() -> Self {
        Self {
            tags: [0; 8],
            data: [(0, V::default()); 8],
        }
    }
}
