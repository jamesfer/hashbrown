use std::cell::UnsafeCell;
use std::cmp::{max, min};
use std::ops::{Deref, Range};
use std::sync::{Mutex, TryLockError};

#[derive(Debug)]
pub enum ClaimOnceError {
    Poisoned,
    AlreadyClaimed,
}

pub struct ClaimOnce<T> {
    value: Mutex<Option<T>>
}

impl <T> ClaimOnce<T> {
    pub fn new(value: T) -> Self {
        Self {
            value: Mutex::new(Some(value)),
        }
    }

    pub fn claim(&self) -> Result<T, ClaimOnceError> {
        // let is_first = !self.claimed.swap(true, std::sync::atomic::Ordering::Relaxed);
        match self.value.try_lock() {
            Ok(mut maybe_value) => {
                match maybe_value.take() {
                    Some(value) => Ok(value),
                    None => Err(ClaimOnceError::AlreadyClaimed),
                }
            }
            Err(TryLockError::Poisoned(_)) => Err(ClaimOnceError::Poisoned),
            Err(TryLockError::WouldBlock) => Err(ClaimOnceError::AlreadyClaimed),
        }
    }
}

#[repr(transparent)]
pub struct UnsafeCellSendWrapper<T> {
    cell: UnsafeCell<T>
}

unsafe impl <T: Send> Send for UnsafeCellSendWrapper<T> {}
unsafe impl <T: Sync> Sync for UnsafeCellSendWrapper<T> {}

impl <T> UnsafeCellSendWrapper<T> {
    pub fn new(cell: UnsafeCell<T>) -> Self {
        Self { cell }
    }
}

impl <T> Deref for UnsafeCellSendWrapper<T> {
    type Target = UnsafeCell<T>;

    fn deref(&self) -> &Self::Target {
        &self.cell
    }
}

pub fn get_owned_range(
    total_size: usize,
    actor_count: usize,
    actor_index: usize,
) -> (usize, usize) {
    // Determine which indices this thread is responsible for
    let floor_values_per_actor = total_size / actor_count;
    let remainder = total_size % actor_count;
    let start_index = actor_index * floor_values_per_actor + min(actor_index, remainder);
    let end_index = start_index + floor_values_per_actor + (if actor_index < remainder { 1 } else { 0 });
    (start_index, end_index)
}

#[cfg(test)]
mod tests {
    #[test]
    pub fn test_get_owned_range_with_equal_divisions() {
        assert_eq!(super::get_owned_range(12, 3, 0), (0, 4));
        assert_eq!(super::get_owned_range(12, 3, 1), (4, 8));
        assert_eq!(super::get_owned_range(12, 3, 2), (8, 12));
    }

    #[test]
    pub fn test_get_owned_range_with_unequal_divisions() {
        assert_eq!(super::get_owned_range(10, 3, 0), (0, 4));
        assert_eq!(super::get_owned_range(10, 3, 1), (4, 7));
        assert_eq!(super::get_owned_range(10, 3, 2), (7, 10));
    }
}
