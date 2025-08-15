#[cfg(target_os="windows")]
pub mod windows;
#[cfg(target_os="linux")]
pub mod linux;

#[cfg(target_os="windows")]
pub use windows::inner::WinAllocator as InnerAllocator;
#[cfg(target_os="linux")]
pub use linux::inner::LinuxAllocator as InnerAllocator;