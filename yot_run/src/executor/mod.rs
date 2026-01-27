//! The task executor for driving async tasks to completion.
//!
//! The executor manages a pool of worker threads that poll tasks in a work-stealing queue,
//! enabling efficient parallel execution of async workloads.

mod worker;
use self::worker::WorkerHandle;
use crate::task::Task;
use crossbeam_queue::SegQueue;
use metrics::{counter, gauge};
use std::sync::OnceLock;
use std::sync::{Arc, atomic::Ordering};
use std::thread;

/// Global handle for the executor containing the injector queue and worker threads.
#[derive(Debug)]
pub struct ExecutorHandle {
    /// Global task injector queue where tasks can be pushed from anywhere.
    injector: Arc<SegQueue<Arc<Task>>>,
    /// Vector of all worker thread handles.
    workers: Arc<Vec<WorkerHandle>>,
}

impl ExecutorHandle {
    /// Enqueues a task and wakes up an idle worker if available.
    pub fn enqueue(&self, task: Arc<Task>) {
        self.injector.push(task);
        self.try_unpark_one();

        gauge!("yot_run_injector_depth").set(self.injector.len() as f64);
    }

    /// Attempts to wake up one idle worker from all available workers.
    pub fn try_unpark_one(&self) {
        let mut unparked = false;
        for w in self.workers.iter() {
            if w.idle.swap(false, Ordering::Acquire) {
                w.wake();

                // Track how often we successfully wake a sleeping worker
                counter!("yot_run_worker_unparks_total", "worker_id" => w.id.to_string())
                    .increment(1);
                unparked = true;
                break;
            }
        }

        if !unparked {
            // Track "Saturation": We tried to unpark but everyone was already busy
            counter!("yot_run_worker_saturation_events_total").increment(1);
        }
    }
}

/// The executor that manages worker threads and task execution.

pub struct Executor {}

impl Executor {
    /// Creates a new executor with worker threads (capped at 10).
    ///
    /// Returns an `ExecutorHandle` for enqueuing tasks.
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
