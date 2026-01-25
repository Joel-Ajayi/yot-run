# yot-run

A work-stealing, multi-threaded async executor with non-blocking scheduling, OS-backed parking, reactor integration, and lock-free coordination.

## Features

- Work-stealing task scheduler
- Multi-threaded executor with workers
- Non-blocking scheduling and lock-free coordination
- OS-backed parking/unparking for idle threads
- Reactor integration for IO readiness

## Quick start

Build the project:

```bash
cargo build --release
```

Run tests:

```bash
cargo test
```

## Usage

This crate provides a lightweight async executor. See the `src` tree for the executor, reactor, and worker implementations. To run examples or integrate the executor into your application, add this crate as a dependency or run the example binaries (if present) with `cargo run --example <name>`.

## Contributing

Contributions, bug reports, and improvements are welcome — open an issue or submit a pull request.

## License

See `Cargo.toml` for project metadata.
