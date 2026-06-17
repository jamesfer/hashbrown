use std::sync::atomic::AtomicPtr;
use tokio::sync::{Mutex, MutexGuard, TryLockError};

#[derive(Debug, Clone, Copy)]
pub struct Offset {
    pub index: usize,
    pub size: usize,
}

pub struct GlobalIndexTracker {
    value: Mutex<Offset>,
}

impl GlobalIndexTracker {
    pub fn new() -> Self {
        GlobalIndexTracker {
            value: Mutex::new(Offset { index: 0, size: 0 }),
        }
    }

    pub async fn allocate(&self, size: usize) -> Offset {
        let mut value = self.value.lock().await;
        let result = value.clone();
        value.index += 1;
        value.size += size;
        result
    }

    pub fn try_get_current_offset(&self) -> Result<Offset, TryLockError> {
        match self.value.try_lock() {
            Ok(offset) => Ok(*offset),
            Err(err) => Err(err),
        }
    }
}
