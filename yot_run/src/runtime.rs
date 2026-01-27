//! The async runtime managing executor and reactor threads.
//!
//! The runtime coordinates task execution via the executor and I/O event handling via the reactor,
//! storing both in thread-local storage for easy access.

use std::future::{self, Future};
use std::sync::{Arc, OnceLock};
use std::task::Context;
use std::thread;

use crate::executor::{Executor, ExecutorHandle};
use crate::reactor::{run_reactor_loop, Reactor};
use crate::task::Task;

/// Thread-local storage for the current task executor handle.
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
    pub fn new() -> std::io::Result<Self> {
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
