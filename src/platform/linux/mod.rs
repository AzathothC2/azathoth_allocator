use azathoth_core::os::linux::consts::{STDOUT, SYS_WRITE};
use azathoth_core::os::linux::syscalls::syscall3;

pub mod inner;
mod maps;


pub fn write(buf: &str){
 write_bytes(buf.as_bytes());
}  

fn write_bytes(buf: &[u8]) {
 syscall3(SYS_WRITE, STDOUT, buf.as_ptr() as usize, buf.len());
 
}
pub fn writenum(n: u32) {
  let mut buf = [0u8; 10];
  let str = u32_to_str_buf(n, &mut buf);
 write_bytes(str);
}
fn u32_to_str_buf(mut n: u32, buf: &mut [u8; 10]) -> &[u8] {
 let mut i = 10;
 if n == 0 {
  i -= 1;
  buf[i] = b'0';
 } else {
  while n > 0 {
   i -= 1;
   buf[i] = b'0' + (n % 10) as u8;
   n /= 10;
  }
 }
 &buf[i..]
}