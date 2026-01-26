//! A custom async runtime implementation demonstrating the core components of an async executor.
//!
//! This crate provides a lightweight executor for running async tasks, with support for spawning
//! futures and managing their lifecycle. It consists of:
//!
//! - [`executor`]: The main executor for driving tasks to completion
//! - [`reactor`]: Event-driven reactor for handling I/O operations
//! - [`task`]: Task management and state tracking
//! - [`waker`]: Waker implementation for task scheduling

pub mod executor;
pub mod reactor;
pub mod runtime;
pub mod task;
pub mod waker;

pub use yot_run_macros::main;
