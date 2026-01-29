use mio::{Events, Poll, Registry, Token};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::task::Waker;

/// Manages I/O readiness events for async tasks.
///
/// The reactor uses system-level event notification (e.g., epoll on Linux, kqueue on macOS)
/// to efficiently detect when resources become available and notify waiting tasks through their wakers.
///
/// # Architecture
///
/// - **Registry**: Central collection point where sockets are registered for I/O monitoring
/// - **Wakers Map**: Maps event tokens to the wakers that should be invoked when events occur
/// - **Poll Loop**: Background thread that waits for OS events and triggers corresponding wakers
pub(crate) struct Reactor {
    // Shared registry so workers can register their sockets
    pub registry: Registry,
    pub wakers: Arc<Mutex<HashMap<Token, Waker>>>,
}

impl Reactor {
    /// Creates a new reactor with a poll instance.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - `Arc<Reactor>`: The reactor instance for shared ownership
    /// - `Poll`: The OS-level event poller that will be used in the reactor loop
    ///
    /// # Errors
    ///
    /// Returns an IO error if reactor initialization fails.
    pub fn new() -> std::io::Result<(Arc<Self>, Poll)> {
        let poll = Poll::new()?;
        let registry = poll.registry().try_clone()?;
        let reactor = Arc::new(Self {
            registry,
            wakers: Arc::new(Mutex::new(HashMap::new())),
        });

        Ok((reactor, poll))
    }

    /// Associates a waker with a token for future event notifications.
    ///
    /// When the reactor detects an event for the given token, the associated waker
    /// will be invoked to notify the waiting task.
    ///
    /// # Arguments
    ///
    /// * `token` - The event token (typically from a registered socket)
    /// * `waker` - The waker to invoke when the event becomes ready
    pub fn add_waker(&self, token: Token, waker: Waker) {
        let mut wakers = self.wakers.lock().unwrap();
        wakers.insert(token, waker);
    }
}

/// The background event loop that runs in a dedicated thread.
///
/// This function blocks waiting for OS I/O events and wakes corresponding tasks when
/// events become ready. It runs continuously until the program terminates.
///
/// # Arguments
///
/// * `poll` - The OS-level event poller (e.g., epoll instance)
/// * `wakers` - Shared map from event tokens to wakers
///
/// # Panics
///
/// Panics if an unrecoverable polling error occurs (not an interruption).
pub(crate) fn run_reactor_loop(mut poll: Poll, wakers: Arc<Mutex<HashMap<Token, Waker>>>) {
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
