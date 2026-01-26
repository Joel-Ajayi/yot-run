use crate::{executor::ExecutorHandle, task::Task};
use crossbeam_deque::{Steal, Stealer, Worker as DequeWorker};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock,
    },
    thread,
};

#[derive(Debug)]
pub struct WorkerHandle {
    pub id: usize,
    pub stealer: Stealer<Arc<Task>>,
    pub idle: Arc<AtomicBool>,
    thread: thread::Thread,
}

impl WorkerHandle {
    pub fn wake(&self) {
        self.thread.unpark();
    }
}

pub struct Worker {
    pub id: usize,
    local_q: DequeWorker<Arc<Task>>,
    executor_handle: Arc<OnceLock<Arc<ExecutorHandle>>>,
    idle: Arc<AtomicBool>,
}

impl Worker {
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

    fn run(&mut self) {
        // Cache the Arc<Vec<WorkerHandle>> locally for fast access.
        // wait blocks the thread untill .set has been called
        let exector_handle = self.executor_handle.wait();

        loop {
            if let Some(task) = self.local_q.pop() {
                self.poll(task, exector_handle.clone());
                continue;
            }

            // drain injector
            while let Some(task) = exector_handle.injector.pop() {
                self.local_q.push(task);
            }

            if !self.local_q.is_empty() {
                continue;
            }

            // Steal tasks
            let num_workers = exector_handle.workers.len();
            let rand_start_idx = rand::random::<u32>() as usize % num_workers;
            for i in 0..num_workers {
                let idx = (rand_start_idx + i) % num_workers;
                let victim = &exector_handle.workers[idx];

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

            thread::park();

            self.idle.store(false, Ordering::Release);
        }
    }

    fn poll(&self, task: Arc<Task>, executor_handle: Arc<ExecutorHandle>) {
        if let Some(future) = task.try_take() {
            // Add the task back to the queue if pending
            let waker = task.get_or_init_waker(executor_handle);
            task.poll(future, waker);
        }
    }
}
