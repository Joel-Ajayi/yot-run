use sptr::Strict;
use std::future::Future;
use std::pin::Pin;
use std::ptr;
use std::sync::atomic::AtomicUsize;
use std::sync::OnceLock;
use std::sync::{
    atomic::{AtomicPtr, AtomicU8, Ordering},
    Arc,
};
use std::task::{Context, Poll, Waker};

use crate::executor::ExecutorHandle;
use crate::waker;

/// Global counter that starts at 0.
pub(crate) static NEXT_TASK_ID: AtomicUsize = AtomicUsize::new(0);

const TASK_STATE_IDLE: u8 = 0;
const TASK_STATE_POLLING: u8 = 1;
const TASK_STATE_COMPLETED: u8 = 2;

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
    // Cache the waker here to avoid repeated Box allocations
    pub waker: OnceLock<Waker>,
}

impl Task {
    /// Creates a new task from a future.
    ///
    /// The future is stored in an atomic pointer to allow concurrent access from multiple
    /// executor workers. The task is returned wrapped in an `Arc` for reference counting.
    pub fn new(future: TaskFuture) -> Self {
        let id = NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed);

        // Convert the future into a raw pointer, consuming the Box while preserving the heap allocation.
        let ptr = Box::into_raw(Box::new(future));

        Self {
            id,
            state: AtomicU8::new(TASK_STATE_IDLE),
            future: AtomicPtr::new(ptr),
            waker: OnceLock::new(),
        }
    }

    pub fn try_take(&self) -> Option<Box<TaskFuture>> {
        // Since Polling requires mutational and we can have double or more polling
        // Check if the task is IDLE and mark it as POLLING
        // Use AcqRel to ensure visibility across worker threads

        if self
            .state
            .compare_exchange(
                TASK_STATE_IDLE,
                TASK_STATE_POLLING,
                Ordering::AcqRel,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return None;
        }

        let ptr = self.future.swap(ptr::null_mut(), Ordering::Acquire);

        if ptr.is_null() {
            // Should not happen, but be safe
            self.state.store(TASK_STATE_IDLE, Ordering::Release);
            return None;
        }

        Some(unsafe { Box::from_raw(ptr) })
    }

    pub fn poll(&self, mut future: Box<TaskFuture>, waker: &Waker) {
        let mut ctx = Context::from_waker(waker);
        // Remember: 'future' is already pinned because TaskFuture is Pin<Box<...>>
        match (*future).as_mut().poll(&mut ctx) {
            Poll::Ready(()) => {
                // Task finished: Update state to COMPLETE.
                // The 'future' variable goes out of scope here and is dropped automatically.
                self.state.store(TASK_STATE_COMPLETED, Ordering::Release);
            }
            Poll::Pending => {
                // Task not finished: Put the future back into the mailbox.
                self.release_after_poll(future);
            }
        }
    }

    pub fn get_or_init_waker(self: &Arc<Self>, executor_handle: Arc<ExecutorHandle>) -> &Waker {
        self.waker
            .get_or_init(|| waker::task_waker(self.clone(), executor_handle))
    }

    /// Internal helper to put the future back in the mailbox
    fn release_after_poll(&self, future: Box<TaskFuture>) {
        // Task pending: put the pointer back and reset state to IDLE
        let ptr = Box::into_raw(future);
        // Store the address back so another thread can take it later
        self.future.store(ptr, Ordering::Release);
        // Reset state to IDLE so 'try_take' can succeed again
        self.state.store(TASK_STATE_IDLE, Ordering::Release);
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
