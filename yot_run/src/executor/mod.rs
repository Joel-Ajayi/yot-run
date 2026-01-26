//! The task executor for driving async tasks to completion.
//!
//! The executor manages a pool of worker threads that poll tasks in a work-stealing queue,
//! enabling efficient parallel execution of async workloads.

/// A multi-threaded executor for running async tasks.
///
/// The executor spawns tasks from futures and manages their execution across multiple worker threads.
mod worker;

use self::worker::WorkerHandle;
use crate::task::Task;
use core::num;
use crossbeam_queue::SegQueue;
use std::sync::OnceLock;
use std::thread;
use std::{
    future::Future,
    sync::{atomic::Ordering, Arc},
};

#[derive(Debug)]
pub struct ExecutorHandle {
    injector: Arc<SegQueue<Arc<Task>>>,
    workers: Arc<Vec<WorkerHandle>>,
}

impl ExecutorHandle {
    pub fn enqueue(&self, task: Arc<Task>) {
        self.injector.push(task);
        self.try_unpark_one();
    }

    pub fn try_unpark_one(&self) {
        for w in self.workers.iter() {
            if w.idle.swap(false, Ordering::Acquire) {
                w.wake();
                break;
            }
        }
    }
}

pub struct Executor {
    pub handle: Arc<ExecutorHandle>,
}

impl Executor {
    pub fn new() -> std::io::Result<Arc<ExecutorHandle>> {
        let mut num_workers = thread::available_parallelism()?.get();
        num_workers = if num_workers > 10 { 10 } else { num_workers };

        let injector = Arc::new(SegQueue::new());
        let executor_handle = Arc::new(OnceLock::<Arc<ExecutorHandle>>::new());

        let mut worker_handles = Vec::with_capacity(num_workers);

        // create worker handles (threads started in Worker::start)
        for id in 0..num_workers {
            worker_handles.push(worker::Worker::start(id, executor_handle.clone()));
        }

        let shared_workers = Arc::new(worker_handles);
        let executor_handle_final = Arc::new(ExecutorHandle {
            injector,
            workers: shared_workers,
        });

        executor_handle
            .set(executor_handle_final.clone())
            .expect("Failed to initialize workers");

        Ok(executor_handle_final)
    }
}
