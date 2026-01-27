use std::future::{self, Future};
use std::sync::{Arc, OnceLock};
use std::task::Context;
use std::thread;

use crate::executor::{Executor, ExecutorHandle};
use crate::reactor::{run_reactor_loop, Reactor};
use crate::task::Task;

// const: This allows the compiler to initialize the variable at compile time instead of at runtime.
thread_local! {
    static HANDLE: OnceLock<Arc<ExecutorHandle>> = const { OnceLock::new() };
    static REACTOR: OnceLock<Arc<Reactor>> = const { OnceLock::new() };
}

pub struct Runtime {
    handle: Arc<ExecutorHandle>,
    reactor: Arc<Reactor>,
}

impl Runtime {
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

    pub fn enter(&self) {
        HANDLE.with(|h| {
            let _ = h.set(self.handle.clone());
        });
        REACTOR.with(|r| {
            let _ = r.set(self.reactor.clone());
        });
    }

    pub fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let task = Arc::new(Task::new(Box::pin(fut)));
        self.handle.enqueue(task);
    }

    // This worker thread
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

pub(crate) fn get_handle() -> Arc<ExecutorHandle> {
    HANDLE.with(|h| {
        h.get()
            .cloned()
            .expect("yot_run: spawn called outside of a runtime context")
    })
}

/// Used by TcpListener::bind() to find the reactor in TLS.
pub(crate) fn get_reactor() -> Arc<Reactor> {
    REACTOR.with(|r| {
        r.get()
            .cloned()
            .expect("yot_run: I/O type used outside of a runtime context")
    })
}
