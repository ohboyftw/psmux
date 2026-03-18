// lib.rs — expose modules for integration tests while keeping the
// primary binary entry-point in main.rs.
#![allow(dead_code)]

pub mod backend;
pub mod remote;
pub mod session;
pub mod types;
