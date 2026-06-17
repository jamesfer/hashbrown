use core::{fmt, mem};

/// Single tag in a control group.
#[derive(Copy, Clone, PartialEq, Eq)]
#[repr(transparent)]
pub(crate) struct Tag(pub u8);
impl Tag {
    /// Control tag value for an empty bucket.
    pub(crate) const EMPTY: Tag = Tag(0b0000_0000);

    /// Control tag value for a deleted bucket.
    pub(crate) const DELETED: Tag = Tag(0b0000_0001);

    /// Checks whether a control tag represents a full bucket.
    #[inline]
    pub(crate) const fn is_full(self) -> bool {
        self.0 > Self::DELETED.0
    }

    /// Checks whether a control tag represents a special value.
    #[inline]
    pub(crate) const fn is_special(self) -> bool {
        self.0 <= Self::DELETED.0
    }

    /// Checks whether a special control value is EMPTY (just check 1 bit).
    #[inline]
    pub(crate) const fn special_is_empty(self) -> bool {
        debug_assert!(self.is_special());
        self.0 & 0x01 == 0
    }

    /// Creates a control tag representing a full bucket with the given hash.
    #[inline]
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn full(hash: u64) -> Tag {
        // Constant for function that grabs the top 8 bits of the hash.
        const MIN_HASH_LEN: usize = if mem::size_of::<usize>() < mem::size_of::<u64>() {
            mem::size_of::<usize>()
        } else {
            mem::size_of::<u64>()
        };

        // Grab the top 8 bits of the hash. While the hash is normally a full 64-bit
        // value, some hash functions (such as FxHash) produce a usize result
        // instead, which means that the top 32 bits are 0 on 32-bit platforms.
        // So we use MIN_HASH_LEN constant to handle this.
        let top7 = hash >> (MIN_HASH_LEN * 8 - 8);
        Tag((top7 as u8).max(Self::DELETED.0 + 1)) // truncation
    }
}
impl fmt::Debug for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_special() {
            if self.special_is_empty() {
                f.pad("EMPTY")
            } else {
                f.pad("DELETED")
            }
        } else {
            f.debug_tuple("full").field(&(self.0 & 0x7F)).finish()
        }
    }
}

/// Extension trait for slices of tags.
pub(crate) trait TagSliceExt {
    /// Fills the control with the given tag.
    fn fill_tag(&mut self, tag: Tag);

    /// Clears out the control.
    #[inline]
    fn fill_empty(&mut self) {
        self.fill_tag(Tag::EMPTY)
    }
}
impl TagSliceExt for [Tag] {
    #[inline]
    fn fill_tag(&mut self, tag: Tag) {
        // SAFETY: We have access to the entire slice, so, we can write to the entire slice.
        unsafe { self.as_mut_ptr().write_bytes(tag.0, self.len()) }
    }
}
