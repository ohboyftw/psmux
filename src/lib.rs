// lib.rs — expose modules for integration tests while keeping the
// primary binary entry-point in main.rs.
#![allow(dead_code)]

pub mod backend;
pub mod control;
pub mod crash;
pub mod debug_log;
pub mod hints;
#[cfg(feature = "mycel")]
pub mod mycel;
pub mod octal;
pub mod orchestrate;
pub mod remote;
pub mod session;
pub mod types;
pub mod wait_for;
