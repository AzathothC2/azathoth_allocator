#![no_std]

mod platform;
#[cfg(feature="multithread")]
pub mod lock;

pub const MAX_RECORDS: usize = 512;

pub mod allocator;
mod base;
mod metadata;
