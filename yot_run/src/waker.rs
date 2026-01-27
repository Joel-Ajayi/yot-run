//! Waker implementations for task scheduling and notification.
//!
//! This module provides two types of wakers:
//! - **Task Waker**: Re-enqueues tasks into the executor's work queue
//! - **Unpark Waker**: Wakes parked threads for synchronous blocking operations

use crate::executor::ExecutorHandle;
use crate::task::Task;
use std::sync::Arc;
use std::task::{RawWaker, RawWakerVTable, Waker};
use std::thread::Thread;

/// Wakes a task to resume execution.
///
/// When a task is waiting on I/O or other events, the waker is used to notify the executor
/// that the task is ready to progress and should be polled again.

/// Data associated with a task waker.
///
/// Contains references to the task and the executor handle needed to re-enqueue
/// the task when the waker is triggered.
///
/// # Invariants
///
/// This structure is allocated on the heap and managed through raw pointers by
/// the RawWaker machinery. The `task` and `handle` are kept alive via `Arc` reference
/// counts managed by the waker vtable functions.
pub struct WakerData {
    task: Arc<Task>,
    handle: Arc<ExecutorHandle>,
}

/// Creates a waker that re-enqueues a task into the executor.
///
/// This waker is used for tasks being executed by background workers. When triggered,
/// it pushes the task back onto the executor's global injector queue.
///
/// # Arguments
///
/// * `task` - The task to wake
/// * `handle` - The executor handle for re-enqueueing
///
/// # Returns
///
/// A `Waker` that can be passed to futures and will enqueue the task when called.
pub fn task_waker(task: Arc<Task>, handle: Arc<ExecutorHandle>) -> Waker {
    let data = Box::new(WakerData { task, handle });
    let ptr = Box::into_raw(data) as *const ();
    unsafe { Waker::from_raw(RawWaker::new(ptr, &VTABLE)) }
}

unsafe fn clone(data: *const ()) -> RawWaker {
    // Cast the pointer back to a reference (do not take ownership!)
    let data = unsafe { &*(data as *const WakerData) };
    let cloned = Box::new(WakerData {
        task: data.task.clone(),
        handle: data.handle.clone(),
    });
    RawWaker::new(Box::into_raw(cloned) as *const (), &VTABLE)
}

unsafe fn wake(data: *const ()) {
    // Take ownership of the Box so it drops at the end of this function
    let data = unsafe { Box::from_raw(data as *mut WakerData) };
    data.handle.enqueue(data.task);
}

unsafe fn wake_by_ref(data: *const ()) {
    // Cast the pointer back to a reference (do not take ownership!)
    let data = unsafe { &*(data as *const WakerData) };
    data.handle.enqueue(data.task.clone());
}

unsafe fn drop(data: *const ()) {
    // reclaim the Box and let it drop naturally
    let _ = unsafe { Box::from_raw(data as *mut WakerData) };
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);

/// Creates a waker that unparks a specific thread.
///
/// This waker is used for synchronous blocking operations like `block_on`. When triggered,
/// it unparks the associated thread, allowing it to resume execution.
///
/// # Arguments
///
/// * `thread` - The thread handle to unpark when the waker is triggered
///
/// # Returns
///
/// A `Waker` that can be passed to futures and will unpark the thread when called.
pub fn unpark_waker(thread: Thread) -> Waker {
    let ptr = Box::into_raw(Box::new(thread)) as *const ();
    unsafe { Waker::from_raw(RawWaker::new(ptr, &UNPARK_VTABLE)) }
}

unsafe fn clone_unpark(ptr: *const ()) -> RawWaker {
    let thread = unsafe { (*(ptr as *const Thread)).clone() };
    RawWaker::new(Box::into_raw(Box::new(thread)) as *const (), &UNPARK_VTABLE)
}

unsafe fn wake_unpark(ptr: *const ()) {
    let thread = unsafe { *Box::from_raw(ptr as *mut Thread) };
    thread.unpark();
}

unsafe fn wake_unpark_by_ref(ptr: *const ()) {
    let thread = unsafe { &*(ptr as *const Thread) };
    thread.unpark();
}

unsafe fn drop_unpark(ptr: *const ()) {
    let _ = unsafe { Box::from_raw(ptr as *mut Thread) };
}

static UNPARK_VTABLE: RawWakerVTable =
    RawWakerVTable::new(clone_unpark, wake_unpark, wake_unpark_by_ref, drop_unpark);
