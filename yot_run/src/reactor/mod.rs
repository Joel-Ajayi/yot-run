//! Event-driven I/O reactor.
//!
//! The reactor monitors system events and notifies tasks when they are ready to progress.

pub mod reactor;
pub use reactor::{run_reactor_loop, Reactor};
