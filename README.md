# yot-run

[![Crates.io](https://img.shields.io/crates/v/yot_run.svg)](https://crates.io/crates/yot_run)
[![Docs.rs](https://docs.rs/yot_run/badge.svg)](https://docs.rs/yot_run/)
[![License](https://img.shields.io/crates/l/yot_run.svg)](https://github.com/Joel-Ajayi/yot-run#license)
[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)

A custom async runtime implementation demonstrating the core components of an async executor. `yot-run` is an educational project that provides a lightweight executor for running async tasks with support for spawning futures, managing their lifecycle, and integrating with OS-level I/O events.

## Overview

`yot-run` implements a complete async runtime from the ground up, including:

- **Executor**: A work-stealing task scheduler with multiple worker threads
- **Reactor**: OS-backed event notification (epoll/kqueue) for async I/O
- **Task Management**: Thread-safe task lifecycle tracking using atomic operations
- **Waker System**: Custom waker implementations for task scheduling and thread unparking
- **Networking**: Async TCP primitives (`TcpListener`, `TcpStream`) built on top of the runtime

## Features

- ✨ **Work-stealing task scheduler** - Efficient task distribution across worker threads
- 🔄 **Multi-threaded executor** - CPU-aware worker pool (capped at 10 workers)
- 🚀 **Non-blocking scheduling** - Lock-free coordination using atomic operations
- 😴 **OS-backed parking** - Idle threads sleep without busy-waiting
- 🌐 **Reactor integration** - Event-driven I/O with epoll/kqueue support
- 📊 **Metrics tracking** - Built-in Prometheus metrics (optional)
- 🎯 **Macro support** - `#[yot_run::main]` attribute for easy setup

## Architecture

### Core Components

```
┌─────────────────────────────────────────┐
│         Runtime (main coordinator)      │
├─────────────────────────────────────────┤
│                                         │
│  ┌─────────────┐      ┌──────────────┐ │
│  │  Executor   │      │   Reactor    │ │
│  │             │      │              │ │
│  │ ┌─────────┐ │      │ ┌──────────┐ │ │
│  │ │ Worker1 │ │      │ │ epoll/   │ │ │
│  │ │ Worker2 │ │      │ │  loop    │ │ │
│  │ │ Worker3 │ │      │ │          │ │ │
│  │ └─────────┘ │      │ └──────────┘ │ │
│  │             │      │              │ │
│  └─────────────┘      └──────────────┘ │
│         ▲                     ▲         │
│         └─────────────────────┘         │
│         Task queues & wakers            │
└─────────────────────────────────────────┘
```

**Runtime**: Coordinates executor and reactor threads, stored in thread-local storage for easy access

**Executor**: Manages a pool of worker threads that steal tasks from a global injector queue and from each other's local queues

**Reactor**: Runs in a background thread monitoring OS I/O events, wakes tasks when data is ready

**Tasks**: Pinned futures with atomic state tracking (idle/polling/completed)

**Wakers**: Two implementations—task wakers re-enqueue into executor, unpark wakers wake threads

### Module Structure

```
yot_run/
├── executor/          # Task scheduling and worker threads
│   ├── mod.rs        # ExecutorHandle and Executor
│   └── worker.rs     # Worker thread implementation
├── reactor/          # I/O event handling
│   ├── mod.rs        # Reactor module definition
│   └── reactor.rs    # Poll loop and event management
├── lib.rs            # Crate documentation and exports
├── runtime.rs        # Runtime orchestration
├── task.rs           # Task definition and lifecycle
├── waker.rs          # Waker implementations
└── net.rs            # Async TCP primitives
```

## Quick Start

### Basic Setup

```rust
#[yot_run::main]
async fn main() {
    println!("Hello from async main!");
}
```

The `#[yot_run::main]` macro automatically:

1. Initializes the runtime
2. Sets up executor and reactor threads
3. Executes your async code within the runtime context

### Spawning Tasks

```rust
#[yot_run::main]
async fn main() {
    yot_run::spawn(async {
        println!("Task running in background");
    });
}
```

### Async I/O

```rust
use yot_run;
use yot_run::net::TcpListener;

#[yot_run::main]
async fn main() -> std::io::Result<()> {
    let listener = TcpListener::bind("127.0.0.1:8080".parse()?)?;

    loop {
        let (stream, addr) = listener.accept().await?;
        yot_run::spawn(async move {
            let mut buf = [0u8; 1024];
            loop {
                let n = match socket.read(&mut buf).await {
                     Ok(n) if n == 0 => return, // Connection closed
                     Ok(n) => n,
                     Err(e) => {
                        eprintln!("Failed to read from socket; err = {:?}", e);
                        return;
                    }
                };

                if let Err(e) = socket.write_all(&buf[0..n]).await {
                     eprintln!("Failed to write to socket; err = {:?}", e);
                     return;
                }
            }
        });
    }
}
```

### Disable Metrics

By default, the runtime starts a Prometheus metrics exporter on port 9000. To disable:

```rust
#[yot_run::main(show_ui = false)]
async fn main() {
    // Metrics disabled
}
```

## Building & Running

Build the project:

```bash
cargo build --release
```

Build documentation:

```bash
cargo doc --no-deps --open
```

Run tests:

```bash
cargo test
```

## How It Works

### Task Execution Flow

1. **Spawn**: Task created and enqueued to executor's injector queue
2. **Worker Processing**: Worker polls local queue, then global injector, then steals from others
3. **Polling**: Future advanced via `poll()` within waker context
4. **Pending**: If future returns `Poll::Pending`, task returned to queue with stored future
5. **Ready**: If future returns `Poll::Ready`, task marked as completed

### I/O Readiness Flow

1. **Register**: Socket registered with reactor via `mio`, assigned a token
2. **Block**: OS blocks in `poll()` waiting for events
3. **Event**: OS signals data ready for socket
4. **Wake**: Reactor looks up waker for token and calls `wake()`
5. **Resume**: Task re-enqueued and resumed by worker

### Synchronous Blocking

`block_on()` parks the current thread and uses an "unpark waker" that unblocks it when the future completes, allowing synchronous code to wait for async operations.

## Metrics

When enabled (`show_ui = true`), metrics are available at `http://localhost:9000/metrics`:

- `yot_run_tasks_spawned_total` - Total tasks spawned
- `yot_run_tasks_pending_current` - Currently pending tasks
- `yot_run_injector_depth` - Tasks in global queue
- `yot_run_worker_saturation_events_total` - Times all workers were busy
- `yot_run_worker_unparks_total` - Worker wake events by thread

## Contributing

Contributions, bug reports, and improvements are welcome — open an issue or submit a pull request.

## License

See `Cargo.toml` for project metadata.

## Educational Purpose

This project is designed to teach and demonstrate:

- How async executors work internally
- Work-stealing scheduling algorithms
- Thread synchronization with atomic operations
- OS-level I/O event handling with epoll/kqueue
- Waker-based task notification systems
- Thread parking for efficient idle waiting
