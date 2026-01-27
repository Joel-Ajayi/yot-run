use mio::{Events, Interest, Poll, Registry, Token};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::task::Waker;

/// Manages I/O readiness events for async tasks.
///
/// The reactor uses system-level event notification (e.g., epoll, kqueue) to efficiently
/// detect when resources become available and notify waiting tasks.
pub struct Reactor {
    // Shared registry so workers can register their sockets
    pub registry: Registry,
    pub wakers: Arc<Mutex<HashMap<Token, Waker>>>,
}

impl Reactor {
    pub fn new() -> std::io::Result<(Arc<Self>, Poll)> {
        let mut poll = Poll::new()?;
        let registry = poll.registry().try_clone()?;
        let reactor = Arc::new(Self {
            registry,
            wakers: Arc::new(Mutex::new(HashMap::new())),
        });

        Ok((reactor, poll))
    }

    pub fn add_waker(&self, token: Token, waker: Waker) {
        let mut wakers = self.wakers.lock().unwrap();
        wakers.insert(token, waker);
    }
}

/// The background event loop.
/// This stays in its own thread to avoid blocking worker threads.
pub fn run_reactor_loop(mut poll: Poll, wakers: Arc<Mutex<HashMap<Token, Waker>>>) {
    let mut events = Events::with_capacity(1024);

    loop {
        // Block here until the OS signals readiness. 0% CPU while waiting.
        if let Err(e) = poll.poll(&mut events, None) {
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            panic!("Reactor poll error: {}", e);
        }

        for event in events.iter() {
            let token = event.token();
            let mut wakers_guard = wakers.lock().unwrap();

            // Find the waker associated with this Token
            if let Some(waker) = wakers_guard.remove(&token) {
                // which pushes the task back to the Global Injector.
                waker.wake();
            }
        }
    }
}
