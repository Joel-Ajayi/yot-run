use futures::future::BoxFuture;
use sptr::Strict;
use std::{
    future::{self, Future},
    pin::Pin,
    ptr,
    sync::{
        atomic::{AtomicPtr, AtomicU8, Ordering},
        Arc,
    },
};

const IDLE: u8 = 0;
const POLLING: u8 = 1;
const COMPLETED: u8 = 2;

/// A pinned, heap-allocated future that produces no output.
///
/// `Pin` guarantees that the future's data will not be moved in memory, which is essential for
/// futures that contain self-referential data. While the `Box` container itself can be moved,
/// the data within it remains at a stable heap address, and the data can never be moved out of
/// the `Box`.
pub type TaskFuture = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// Represents an async task managed by the executor.
///
/// A `Task` wraps a future and tracks its execution state using atomic operations to ensure
/// thread-safe access across executor workers.
pub struct Task {
    /// Unique identifier for this task.
    pub id: usize,
    /// Current execution state of the task (idle, polling, or completed).
    pub state: AtomicU8,
    /// Raw pointer to the boxed future, stored atomically for thread-safe access.
    pub future: AtomicPtr<TaskFuture>,
}

impl Task {
    /// Creates a new task from a future.
    ///
    /// The future is stored in an atomic pointer to allow concurrent access from multiple
    /// executor workers. The task is returned wrapped in an `Arc` for reference counting.
    pub fn new(id: usize, future: TaskFuture) -> Arc<Self> {
        // Convert the future into a raw pointer, consuming the Box while preserving the heap allocation.
        let ptr = Box::into_raw(Box::new(future));

        Arc::new(Self {
            id,
            state: AtomicU8::new(IDLE),
            future: AtomicPtr::new(ptr),
        })
    }
}

impl Drop for Task {
    /// Safely deallocates the future when the task is dropped.
    ///
    /// Uses atomic swap with acquire-release ordering to synchronize with any threads that
    /// may be concurrently polling the task.
    fn drop(&mut self) {
        // Atomically replace the future pointer with null. Other threads polling may be in progress,
        // so we use AcqRel ordering to establish synchronization boundaries.
        let ptr = self.future.swap(ptr::null_mut(), Ordering::AcqRel);
        if !ptr.is_null() {
            unsafe {
                drop(Box::from_raw(ptr));
            }
        }
    }
}
