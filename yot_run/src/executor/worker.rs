//! Worker thread implementation for the async executor.
//!
//! Each worker thread maintains a local task queue and can steal tasks from other workers
//! to maintain load balancing. Workers process tasks by polling their futures and handling
//! wakeups through the reactor.

use crate::{executor::ExecutorHandle, task::Task};
use crossbeam_deque::{Steal, Stealer, Worker as DequeWorker};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    thread,
};

/// A handle to a worker thread that allows external threads to interact with it.
///
/// This handle provides access to the worker's task stealer queue and allows
/// waking up a parked worker thread.
#[derive(Debug)]
pub struct WorkerHandle {
    /// Unique identifier for this worker thread.
    pub id: usize,
    /// Stealer for this worker's task queue, used by other workers for work stealing.
    pub stealer: Stealer<Arc<Task>>,
    /// Shared flag indicating whether this worker is idle (parked).
    pub idle: Arc<AtomicBool>,
    /// Handle to the actual thread for parking/unparking.
    thread: thread::Thread,
}

impl WorkerHandle {
    /// Wakes up an idle worker thread from its parked state.
    pub fn wake(&self) {
        self.thread.unpark();
    }
}

/// A worker thread that executes tasks from its local queue and steals from other workers.
///
/// The worker implements a work-stealing scheduler: it first processes its own local queue,
/// then drains the global injector queue, then attempts to steal tasks from other workers.
/// When idle, it parks the thread and waits to be woken up by the reactor or other workers.
pub struct Worker {
    /// Unique identifier for this worker thread.
    pub id: usize,
    /// Local FIFO queue of tasks for this worker.
    local_q: DequeWorker<Arc<Task>>,
    /// Shared executor handle, populated once the executor is fully initialized.
    executor_handle: Arc<OnceLock<Arc<ExecutorHandle>>>,
    /// Shared flag indicating whether this worker is idle.
    idle: Arc<AtomicBool>,
}

impl Worker {
    /// Starts a new worker thread and returns a handle to interact with it.
    ///
    /// This spawns a new OS thread that will continuously run the worker's main loop,
    /// processing tasks from its queue, the injector, and stealing from other workers.
    ///
    /// # Arguments
    ///
    /// * `id` - Unique identifier for this worker
    /// * `executor_handle` - Shared executor state (populated once initialization is complete)
    ///
    /// # Returns
    ///
    /// A `WorkerHandle` that can be used to wake the worker and steal from its queue.
    pub fn start(id: usize, executor_handle: Arc<OnceLock<Arc<ExecutorHandle>>>) -> WorkerHandle {
        let local_q: DequeWorker<Arc<Task>> = DequeWorker::new_fifo();
        let stealer = local_q.stealer();

        let idle_flag = Arc::new(AtomicBool::new(false));
        let idle_flag_clone = idle_flag.clone();

        let builder = thread::Builder::new().name(format!("worker-{id}"));
        let thread_handle = builder
            .spawn(move || {
                let mut worker = Worker {
                    id,
                    idle: idle_flag_clone,
                    local_q,
                    executor_handle,
                };
                worker.run();
            })
            .expect("Faild to spawn worker thread!");

        WorkerHandle {
            id,
            idle: idle_flag,
            stealer,
            // For reference types like &Thread, this creates another reference to the same value
            thread: thread_handle.thread().clone(),
        }
    }

    /// The main run loop for the worker thread.
    ///
    /// Implements the work-stealing scheduler:
    /// 1. Process tasks from the local queue
    /// 2. Drain tasks from the global injector queue
    /// 3. Attempt to steal tasks from other workers
    /// 4. Park the thread if no work is available
    /// 5. Resume when woken up by the reactor or other workers
    fn run(&mut self) {
        // Cache the Arc<Vec<WorkerHandle>> locally for fast access.
        // wait blocks the thread untill .set has been called
        let executor_handle = self.executor_handle.wait();

        loop {
            if let Some(task) = self.local_q.pop() {
                self.poll(task, executor_handle.clone());
                continue;
            }

            // drain injector
            while let Some(task) = executor_handle.injector.pop() {
                self.local_q.push(task);
            }

            if !self.local_q.is_empty() {
                continue;
            }

            // Steal tasks
            let num_workers = executor_handle.workers.len();
            let rand_start_idx = rand::random::<u32>() as usize % num_workers;
            for i in 0..num_workers {
                let idx = (rand_start_idx + i) % num_workers;
                let victim = &executor_handle.workers[idx];

                if victim.id == self.id {
                    continue;
                }

                match victim.stealer.steal_batch(&self.local_q) {
                    Steal::Success(_) => {
                        // We found a task! The rest of the batch is already in our self.local
                        break;
                    }
                    Steal::Retry | Steal::Empty => continue, // High contention on this victim, try next or retry
                }
            }

            // continue with other task if any
            if !self.local_q.is_empty() {
                continue;
            }

            self.idle.store(true, Ordering::Release);

            // Recheck to avoid a lost wakeup.
            if !self.local_q.is_empty() || !executor_handle.injector.is_empty() {
                self.idle.store(false, Ordering::Release);
                continue;
            }

            thread::park();

            self.idle.store(false, Ordering::Release);
        }
    }

    /// Polls a task's future with a waker, handling the result.
    ///
    /// If the task's future is still available (not yet consumed), it polls the future
    /// with a waker and lets the task handle the result internally.
    ///
    /// # Arguments
    ///
    /// * `task` - The task to poll
    /// * `executor_handle` - The executor handle for creating a waker
    fn poll(&self, task: Arc<Task>, executor_handle: Arc<ExecutorHandle>) {
        if let Some(future) = task.try_take() {
            // Add the task back to the queue if pending
            let waker = task.get_or_init_waker(executor_handle);
            task.poll(future, waker);
        }
    }
}
