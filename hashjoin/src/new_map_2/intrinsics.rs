pub fn u8x16_compare_eq_fallback(search: u8, values: &[u8; 16]) -> [u8; 16] {
    let mut output = [0u8; 16];
    for (value, output) in values.iter().zip(output.iter_mut()) {
        *output = u8::from(*value == search) * 255;
    }
    output
}

#[cfg(target_arch = "aarch64")]
pub fn u8x16_compare_eq_aarch64(search: u8, values: &[u8; 16]) -> u128 {
    use std::arch::aarch64::{vceqq_u8, vld1q_dup_u8, vld1q_u8, vgetq_lane_u64, vreinterpretq_u64_u8};

    unsafe {
        // Replicate the search value 16 times into a 128-bit register
        let search_register = vld1q_dup_u8(&search);

        // Load the 16 values from the buffer into a single 128-bit register
        // TODO check if using vld1q_lane_u8 to load the atomic values directly into a register is
        //  better
        let values_register = vld1q_u8(&values[0]);

        // Compare the registers together. For each u8 value in the 128-bit register, if the values
        // match, the output will have all 1s, otherwise all 0s.
        let output = vceqq_u8(values_register, search_register);

        // Reinterpret the 16-lane u8 type into a 2-lane 64-bit type, and return it as a regular
        // rust type
        let uint64x2_value = vreinterpretq_u64_u8(output);
        let output_int = (
            (vgetq_lane_u64::<0>(uint64x2_value) as u128)
                | ((vgetq_lane_u64::<1>(uint64x2_value) as u128) << 64)
        );

        output_int
    }
}

#[inline]
#[cfg(target_arch = "aarch64")]
pub fn u8x16_compare_eq_aarch64_shrink(search: u8, values: &[u8; 16]) -> u64 {
    use std::arch::aarch64::{vceqq_u8, vld1q_dup_u8, vld1q_u8, vget_lane_u64, vreinterpret_u64_u8, vqrshrn_n_u16, vqshrn_n_u16, vreinterpretq_u16_u8, vshrn_n_u16};

    unsafe {
        // Replicate the search value 16 times into a 128-bit register
        let search_register = vld1q_dup_u8(&search);

        // Load the 16 values from the buffer into a single 128-bit register
        // TODO check if using vld1q_lane_u8 to load the atomic values directly into a register is
        //  better
        let values_register = vld1q_u8(&values[0]);

        // Compare the registers together. For each u8 value in the 128-bit register, if the values
        // match, the output will have all 1s, otherwise all 0s.
        let output = vceqq_u8(values_register, search_register);

        let shifted = vshrn_n_u16::<4>(vreinterpretq_u16_u8(output));

        // Reinterpret the 16-lane u8 type into a 2-lane 64-bit type, and return it as a regular
        // rust type
        let output_int = vget_lane_u64::<0>(vreinterpret_u64_u8(shifted));

        output_int
    }
}

#[cfg(target_arch = "aarch64")]
pub fn u8x8_compare_eq_aarch64(search: u8, values: &[u8; 8]) -> u64 {
    use std::arch::aarch64::{vceq_u8, vld1_dup_u8, vld1_u8, vget_lane_u64, vreinterpret_u64_u8};

    unsafe {
        // Replicate the search value 16 times into a 128-bit register
        let search_register = vld1_dup_u8(&search);

        // Load the 16 values from the buffer into a single 128-bit register
        // TODO check if using vld1q_lane_u8 to load the atomic values directly into a register is
        //  better
        let values_register = vld1_u8(&values[0]);

        // Compare the registers together. For each u8 value in the 128-bit register, if the values
        // match, the output will have all 1s, otherwise all 0s.
        let output = vceq_u8(values_register, search_register);

        // Reinterpret the 16-lane u8 type into a 2-lane 64-bit type, and return it as a regular
        // rust type
        let uint64x2_value = vreinterpret_u64_u8(output);
        let output_int = vget_lane_u64::<0>(uint64x2_value);

        output_int
    }
}


#[cfg(target_arch = "aarch64")]
mod tests {
    use crate::new_map_2::intrinsics::{u8x16_compare_eq_aarch64, u8x16_compare_eq_aarch64_shrink};

    #[test]
    pub fn test_u8x16_compare_eq() {
        let search_tag = 0b1010_1010;
        let mut values = [0u8; 16];
        values[0] = 0b1111_0000;
        values[1] = search_tag;
        values[2] = search_tag;
        values[4] = 0b0000_1111;
        values[6] = 0b0011_1111;
        values[10] = 0b1111_1111;
        values[14] = 0b1000_1000;
        values[15] = search_tag;

        let output = u8x16_compare_eq_aarch64(search_tag, &values);

        // Output is reversed, index 0 is stored in the most significant bits
        let expected = 0xFF00_0000_0000_0000_0000_0000_00FF_FF00u128;
        assert_eq!(output, expected, "Output {:032X?}, expected {:032X?}", output, expected);
    }

    #[test]
    pub fn test_u8x16_shrink_compare_eq() {
        let search_tag = 0b1010_1010;
        let mut values = [0u8; 16];
        values[0] = 0b1111_0000;
        values[1] = search_tag;
        values[2] = search_tag;
        values[4] = 0b0000_1111;
        values[6] = 0b0011_1111;
        values[10] = 0b1111_1111;
        values[14] = 0b1000_1000;
        values[15] = search_tag;

        let output = u8x16_compare_eq_aarch64_shrink(search_tag, &values);

        // Output is reversed, index 0 is stored in the most significant bits
        // let original = 0xFF00_0000_0000_0000_0000_0000_00FF_FF00u128;
        // let shifted_ = 0x0FF0_0000_0000_0000_0000_0000_000F_0FF0u128;
        let expected = 0xF000_0000_0000_0FF0u64;
        assert_eq!(output, expected, "Output {:016X?}, expected {:016X?}", output, expected);
    }
}
