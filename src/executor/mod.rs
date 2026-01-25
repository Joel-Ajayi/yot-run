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
    injector: Arc<SegQueue<Arc<Task>>>,
    workers: Arc<Vec<WorkerHandle>>,
}

impl Executor {
    pub fn new() -> Self {
        let num_workers = 10;
        let injector = Arc::new(SegQueue::new());
        let mut handles = Vec::with_capacity(num_workers);
        let workers = Arc::new(handles);
        // create worker handles (threads started in Worker::start)
        for id in 0..num_workers {
            handles.push(worker::Worker::start(id, injector, workers));
        }

        let workers = Arc::new(handles);

        let exec = Executor {
            injector: injector.clone(),
            workers: workers.clone(),
        };
        exec
    }

    pub fn spawn(&self, fut: impl Future<Output = ()> + Send + 'static) {
        let task = Arc::new(Task::new(Box::pin(fut)));
        self.injector.push(task);
        let handle = ExecutorHandle {
            injector: self.injector.clone(),
            workers: self.workers.clone(),
        };
        handle.try_unpark_one();
    }
}
