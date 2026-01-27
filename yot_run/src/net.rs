use mio::{Interest, Token};
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::runtime; // Accesses get_reactor() via TLS
use crate::task::NEXT_TASK_ID;

pub struct TcpListener {
    net: mio::net::TcpListener,
    token: Token,
}

impl TcpListener {
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

    pub async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        AcceptFuture { listener: self }.await
    }
}

/// The LEAF FUTURE for Accept
struct AcceptFuture<'a> {
    listener: &'a TcpListener,
}

// Register your TcpListener with a Poll instance.
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

pub struct TcpStream {
    net: mio::net::TcpStream,
    token: Token,
}

impl TcpStream {
    pub async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        ReadFuture { socket: self, buf }.await
    }
}

/// The LEAF FUTURE for Read
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
