//! Async TCP networking primitives built on top of the custom runtime.
//!
//! This module provides `TcpListener` and `TcpStream` types that integrate
//! with the runtime's reactor to enable non-blocking I/O operations. These
//! types use `mio` for OS-level I/O multiplexing and register themselves
//! with the reactor to receive wakeups when I/O is ready.

use mio::{Interest, Token};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::runtime; // Accesses get_reactor() via TLS
use crate::task::NEXT_TASK_ID;

/// An async TCP listener bound to a local address.
///
/// This listener accepts incoming TCP connections and returns `TcpStream`
/// instances. The listener is registered with the reactor and will wake up
/// waiting tasks when connections are available.
pub struct TcpListener {
    net: mio::net::TcpListener,
    token: Token,
}

impl TcpListener {
    /// Binds a TCP listener to the given address.
    ///
    /// # Arguments
    ///
    /// * `addr` - The socket address to bind to
    ///
    /// # Returns
    ///
    /// Returns `Ok(TcpListener)` on success, or an IO error if binding fails.
    /// The listener is automatically registered with the reactor.
    pub fn bind(addr: SocketAddr) -> io::Result<Self> {
        let mut net = mio::net::TcpListener::bind(addr)?;

        // 1. Generate a unique Token for the Reactor to track
        let token = Token(NEXT_TASK_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed));

        // 2. Grab the Reactor from TLS and register the socket with the OS
        let reactor = runtime::get_reactor();
        reactor
            .registry
            .register(&mut net, token, Interest::READABLE)?;

        Ok(Self { net, token })
    }

    /// Accepts a new incoming TCP connection.
    ///
    /// This is an async operation that suspends the current task until
    /// a connection is available. Returns a `TcpStream` and the remote
    /// socket address.
    ///
    /// # Returns
    ///
    /// Returns `Ok((stream, addr))` when a connection is accepted, or
    /// an IO error on failure.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let listener = TcpListener::bind("127.0.0.1:8080".parse()?)?;
    /// let (stream, addr) = listener.accept().await?;
    /// println!("Accepted connection from {}", addr);
    /// ```
    pub async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        AcceptFuture { listener: self }.await
    }
}

/// A future that completes when a TCP connection is accepted.
///
/// This is the "leaf future" that polls the underlying `mio::net::TcpListener`
/// and coordinates with the reactor to receive wakeups when a connection
/// is ready to accept.
struct AcceptFuture<'a> {
    listener: &'a TcpListener,
}

/// The LEAF FUTURE for Accept
impl Future for AcceptFuture<'_> {
    type Output = io::Result<(TcpStream, SocketAddr)>;

    // Call accept() to obtain a TcpStream.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.listener.net.accept() {
            Ok((mut stream, addr)) => {
                // When a new stream is accepted, we must also register IT with the Reactor
                let token = Token(NEXT_TASK_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
                let reactor = runtime::get_reactor();

                // Register for both Read and Write events
                reactor.registry.register(
                    &mut stream,
                    token,
                    Interest::READABLE | Interest::WRITABLE,
                )?;

                Poll::Ready(Ok((TcpStream { net: stream, token }, addr)))
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // LEAF LOGIC: Leave our Waker at the Reactor "Front Desk"
                let reactor = runtime::get_reactor();
                reactor.add_waker(self.listener.token, cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

/// An async TCP stream connected to a remote peer.
///
/// Provides async read operations. The stream is registered with the reactor
/// and will wake up waiting tasks when data is available or the connection
/// is ready for writing.
///
/// # Examples
///
/// ```ignore
/// let (stream, _) = listener.accept().await?;
/// let mut buf = [0u8; 1024];
/// let n = stream.read(&mut buf).await?;
/// println!("Read {} bytes", n);
/// ```
pub struct TcpStream {
    net: mio::net::TcpStream,
    token: Token,
}

impl TcpStream {
    /// Reads data from the TCP stream asynchronously.
    ///
    /// Suspends the current task until data is available to read. If the read would block,
    /// the reactor will wake the task when data becomes available.
    ///
    /// # Arguments
    ///
    /// * `buf` - The buffer to read data into
    ///
    /// # Returns
    ///
    /// Returns `Ok(n)` with the number of bytes read, or an IO error
    /// if reading fails. Returns `Ok(0)` if the connection is closed.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let mut buf = [0u8; 256];
    /// match stream.read(&mut buf).await {
    ///     Ok(0) => println!("Connection closed"),
    ///     Ok(n) => println!("Read {} bytes", n),
    ///     Err(e) => eprintln!("Read error: {}", e),
    /// }
    /// ```
    pub async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        ReadFuture { socket: self, buf }.await
    }
}

/// A future that completes when data is available to read from a TCP stream.
///
/// This is the "leaf future" that polls the underlying `mio::net::TcpStream`
/// and coordinates with the reactor to receive wakeups when data is ready.
struct ReadFuture<'a> {
    socket: &'a TcpStream,
    buf: &'a mut [u8],
}

impl Future for ReadFuture<'_> {
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        use std::io::Read;
        // Use &* to access the inner stream across the mutable boundary
        match (&self.socket.net).read(self.buf) {
            Ok(n) => Poll::Ready(Ok(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                // LEAF LOGIC: Register this specific stream's token for wakeup
                let reactor = runtime::get_reactor();
                reactor.add_waker(self.socket.token, cx.waker().clone());
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}
