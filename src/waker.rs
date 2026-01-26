use crate::executor::ExecutorHandle;
use crate::task::Task;
use std::sync::Arc;
use std::task::{RawWaker, RawWakerVTable, Waker};

/// Wakes a task to resume execution.
///
/// When a task is waiting on I/O or other events, the waker is used to notify the executor
/// that the task is ready to progress and should be polled again.

pub struct WakerData {
    task: Arc<Task>,
    handle: Arc<ExecutorHandle>,
}

pub fn task_waker(task: Arc<Task>, handle: Arc<ExecutorHandle>) -> Waker {
    let data = Box::new(WakerData { task, handle });
    let ptr = Box::into_raw(data) as *const ();
    unsafe { Waker::from_raw(RawWaker::new(ptr, &VTABLE)) }
}

unsafe fn clone(data: *const ()) -> RawWaker {
    // Cast the pointer back to a reference (do not take ownership!)
    let data = &*(data as *const WakerData);
    let cloned = Box::new(WakerData {
        task: data.task.clone(),
        handle: data.handle.clone(),
    });
    RawWaker::new(Box::into_raw(cloned) as *const (), &VTABLE)
}

unsafe fn wake(data: *const ()) {
    // Take ownership of the Box so it drops at the end of this function
    let data = Box::from_raw(data as *mut WakerData);
    data.handle.enqueue(data.task);
}

unsafe fn wake_by_ref(data: *const ()) {
    // Cast the pointer back to a reference (do not take ownership!)
    let data = &*(data as *const WakerData);
    data.handle.enqueue(data.task.clone());
}

unsafe fn drop(data: *const ()) {
    // reclaim the Box and let it drop naturally
    let _ = Box::from_raw(data as *mut WakerData);
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
