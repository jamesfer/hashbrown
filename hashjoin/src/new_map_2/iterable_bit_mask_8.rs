use std::num::{NonZeroU128, NonZeroU64};
use crate::new_map_2::iterable_bit_mask::IterableBitMaskT;

pub struct IterableBitMask8 {
    value: u64,
}

impl IterableBitMask8 {
    pub fn new(bytes: [u8; 8]) -> Self {
        // Each of the bytes should only ever be a 0 or a 1. We left shift the final value to push
        // the ones to the most significant bit of their byte.
        let value = u64::from_be_bytes(bytes) << 7;
        Self { value }
    }

    pub fn next_set_index(&mut self) -> Option<usize> {
        let value = NonZeroU64::new(self.value)?;

        let leading_zeros = value.leading_zeros();

        // Divide the bit set index by 8 to get a regular index value
        let index = leading_zeros / 8;

        // Remove the top bit from the value
        self.value = self.value & !(1 << (63 - leading_zeros));

        Some(index as usize)
    }
}

impl IterableBitMaskT for IterableBitMask8 {
    fn next_set_index(&mut self) -> Option<usize> {
        self.next_set_index()
    }
}

#[cfg(test)]
mod tests {
    use crate::new_map_2::iterable_bit_mask_8::IterableBitMask8;

    #[test]
    pub fn single_valued_bit_mask() {
        for i in 0..8 {
            let mut data = [0u8; 8];
            data[i] = 1;

            let mut mask = IterableBitMask8::new(data);

            assert_eq!(mask.next_set_index(), Some(i), "Failed for index {}", i);
            assert_eq!(mask.next_set_index(), None);
            assert_eq!(mask.next_set_index(), None);
            assert_eq!(mask.next_set_index(), None);
        }
    }

    #[test]
    pub fn multi_valued_bit_mask() {
        let mut data = [0u8; 8];
        data[0] = 1;
        data[3] = 1;
        data[7] = 1;

        let mut mask = IterableBitMask8::new(data);
        assert_eq!(mask.next_set_index(), Some(0));
        assert_eq!(mask.next_set_index(), Some(3));
        assert_eq!(mask.next_set_index(), Some(7));
        assert_eq!(mask.next_set_index(), None);
        assert_eq!(mask.next_set_index(), None);
        assert_eq!(mask.next_set_index(), None);
    }
}

pub struct IterableBitMaskIntrinsics8 {
    value: u64,
}

impl IterableBitMaskIntrinsics8 {
    pub fn new(value: u64) -> Self {
        // Each of the bytes is either all 1s or all zeros, so we mask the value to only keep the
        // lowest bit per byte. TODO this could be done inside the intrinsics
        Self { value: value & 0x0101_0101_0101_0101u64 }
    }

    pub fn next_set_index(&mut self) -> Option<usize> {
        let value = NonZeroU64::new(self.value)?;
        let zeros = value.trailing_zeros();

        // Remove the lowest bit from the value
        self.value = self.value & (self.value - 1);

        // Divide the bit set index by 8 to get a regular index value
        Some((zeros / 8) as usize)
    }
}

impl IterableBitMaskT for IterableBitMaskIntrinsics8 {
    fn next_set_index(&mut self) -> Option<usize> {
        self.next_set_index()
    }
}

#[cfg(test)]
mod intrinsics_tests {
    use crate::new_map_2::iterable_bit_mask_8::IterableBitMaskIntrinsics8;

    #[test]
    #[cfg(target_arch = "aarch64")]
    pub fn works_with_intrinsics() {
        use crate::new_map_2::intrinsics::u8x8_compare_eq_aarch64;

        let search_tag = 0b1010_1010;
        let mut values = [0u8; 8];
        values[0] = 0b1111_0000;
        values[1] = search_tag;
        values[2] = search_tag;
        values[4] = 0b0000_1111;
        values[6] = 0b0011_1111;
        values[7] = search_tag;

        let num = u8x8_compare_eq_aarch64(search_tag, &values);
        let mut mask = IterableBitMaskIntrinsics8::new(num.clone());

        assert_eq!(mask.next_set_index(), Some(1), "Intrinsic output: {:032X?}", num);
        assert_eq!(mask.next_set_index(), Some(2), "Intrinsic output: {:032X?}", num);
        assert_eq!(mask.next_set_index(), Some(7), "Intrinsic output: {:032X?}", num);
        assert_eq!(mask.next_set_index(), None, "Intrinsic output: {:032X?}", num);
        assert_eq!(mask.next_set_index(), None, "Intrinsic output: {:032X?}", num);
    }
}
