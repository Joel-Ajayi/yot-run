use crate::{task::Task, waker};
use crossbeam_deque::{Injector, Steal, Stealer, Worker as DequeWorker};
use crossbeam_queue::SegQueue;
use std::{
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
    task::Poll,
    thread,
};

static NEXT_WORKER_ID: AtomicUsize = AtomicUsize::new(0);

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
    injector: Arc<SegQueue<Arc<Task>>>,
    workers: Arc<Vec<WorkerHandle>>,
    idle: Arc<AtomicBool>,
}

impl Worker {
    pub fn start(
        injector: Arc<SegQueue<Arc<Task>>>,
        workers: Arc<Vec<WorkerHandle>>,
    ) -> WorkerHandle {
        let id = NEXT_WORKER_ID.fetch_add(1, Ordering::Acquire);
        let local_q: DequeWorker<Arc<Task>> = DequeWorker::new_fifo();
        let stealer = local_q.stealer();

        let idle_flag = Arc::new(AtomicBool::new(false));

        // spawn the thread
        let idle_clone = idle_flag.clone();

        let builder = thread::Builder::new().name(format!("worker-{id}"));
        let thread_handle = builder
            .spawn(move || {
                let mut worker = Worker {
                    id,
                    idle: idle_clone,
                    local_q,
                    injector,
                    workers,
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
        loop {
            if let Some(task) = self.local_q.pop() {
                self.poll(task);
                continue;
            }

            // drain injector
            while let Some(task) = self.injector.pop() {
                self.local_q.push(task);
            }

            if !self.local_q.is_empty() {
                continue;
            }

            // Steal tasks
            let num_workers = self.workers.len();
            let rand_start_idx = rand::random::<u32>() as usize % num_workers;
            for i in 0..num_workers {
                let idx = (rand_start_idx + i) % num_workers;
                let victim = &self.workers[idx];

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

    fn poll(&self, task: Arc<Task>) {
        if let Some(future) = task.take_task() {
            // Add the task back to the queue if pending
            let waker = waker::task_waker(task.clone(), handle.clone());
        }
    }
}
