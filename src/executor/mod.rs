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
use crate::task::NEXT_TASK_ID;
use crossbeam_queue::SegQueue;
use std::sync::OnceLock;
use std::{
    future::Future,
    process::Output,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

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
    handle: Arc<ExecutorHandle>,
}

impl Executor {
    pub fn new() -> Self {
        let num_workers = 10;
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

        executor_handle.set(executor_handle_final.clone());

        let exec = Executor {
            handle: executor_handle_final,
        };
        exec
    }

    pub fn spawn(&self, fut: impl Future<Output = ()> + Send + 'static) {
        let task = Arc::new(Task::new(Box::pin(fut)));
        self.handle.enqueue(task);
    }
}
