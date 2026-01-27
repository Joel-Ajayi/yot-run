//! The async runtime managing executor and reactor threads.
//!
//! The runtime coordinates task execution via the executor and I/O event handling via the reactor,
//! storing both in thread-local storage for easy access.

use std::sync::{Arc, OnceLock};
use std::task::Context;
use std::{io, thread};

use metrics::{counter, gauge};

use crate::executor::{Executor, ExecutorHandle};
use crate::reactor::{Reactor, run_reactor_loop};
use crate::task::Task;

thread_local! {
    static HANDLE: OnceLock<Arc<ExecutorHandle>> = const { OnceLock::new() };
    static REACTOR: OnceLock<Arc<Reactor>> = const { OnceLock::new() };
}

/// The main async runtime combining executor and reactor.
pub struct Runtime {
    /// The task executor handle.
    handle: Arc<ExecutorHandle>,
    /// The I/O reactor handle.
    reactor: Arc<Reactor>,
}

impl Runtime {
    /// Creates and starts a new runtime with executor and reactor threads.
    pub fn new(show_metrics: bool) -> std::io::Result<Self> {
        // Initialize metrics page
        if show_metrics {
            let port = 9000;
            metrics_exporter_prometheus::PrometheusBuilder::new()
                .with_http_listener(([127, 0, 0, 1], port))
                .install()
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
            println!("📊 [yot_run] Metrics enabled at http://localhost:{port}/metrics");
        }

        let (reactor, poll) = Reactor::new()?;
        let wakers = reactor.wakers.clone();
        thread::spawn(move || run_reactor_loop(poll, wakers));

        let executor = Executor::new()?;
        let runtime = Self {
            handle: executor,
            reactor,
        };

        runtime.enter();

        Ok(runtime)
    }

    /// Stores executor and reactor handles in thread-local storage.
    pub fn enter(&self) {
        HANDLE.with(|h| {
            let _ = h.set(self.handle.clone());
        });
        REACTOR.with(|r| {
            let _ = r.set(self.reactor.clone());
        });
    }

    /// Spawns a future to be executed asynchronously.
    pub fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task = Arc::new(Task::new(Box::pin(fut)));
        self.handle.enqueue(task);
    }

    /// Blocks the current thread until the given future completes.
    pub fn block_on<F>(&self, fut: F)
    where
        F: Future<Output = ()>,
    {
        let mut future = Box::pin(fut);
        let thread = thread::current();

        let waker = crate::waker::unpark_waker(thread);
        let mut ctx = Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut ctx) {
                std::task::Poll::Ready(_) => break,
                std::task::Poll::Pending => thread::park(),
            }
        }
    }
}

/// Spawns a future from within an active runtime context.
///
/// # Panics
///
/// Panics if called outside of a runtime context.
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    HANDLE.with(|h| {
        let handle = h
            .get()
            .expect("yot_run: spawn called outside of a runtime context");
        // Track total throughput of the system
        counter!("yot_run_tasks_spawned_total").increment(1);
        gauge!("yot_run_tasks_pending_current").increment(1.0);
        let task = Arc::new(Task::new(Box::pin(future)));

        handle.enqueue(task);
    });
}

/// Retrieves the current runtime's reactor from thread-local storage.
///
/// # Panics
///
/// Panics if called outside of a runtime context.
pub(crate) fn get_reactor() -> Arc<Reactor> {
    REACTOR.with(|r| {
        r.get()
            .cloned()
            .expect("yot_run: I/O type used outside of a runtime context")
    })
}
