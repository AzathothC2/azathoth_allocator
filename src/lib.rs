#![no_std]

mod platform;
pub mod memtrack;

#[cfg(feature="multithread")]
pub mod lock;

pub const MAX_RECORDS: usize = 512;

pub mod allocator;
mod base;
