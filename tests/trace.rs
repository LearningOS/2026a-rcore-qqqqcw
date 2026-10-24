#![allow(dead_code)]
macro_rules! trace { ($($arg:tt)*) => {}; }
mod timer { pub fn get_time_us() -> usize { 1_234_567 } }
mod task {
    pub fn exit_current_and_run_next() { panic!("exit") }
    pub fn suspend_current_and_run_next() {}
    pub fn syscall_count(id: usize) -> isize { if id < 512 { 7 } else { -1 } }
}
#[path = "../os/src/syscall/process.rs"]
mod process;
#[test]
fn trace_byte_access_and_reject_invalid_requests() {
    let mut byte = 0x80u8;
    let address = &mut byte as *mut u8 as usize;
    assert_eq!(process::sys_trace(0, address, 0), 128);
    assert_eq!(process::sys_trace(1, address, 0x1ff), 0);
    assert_eq!(byte, 255);
    assert_eq!(process::sys_trace(2, 410, 0), 7);
    assert_eq!(process::sys_trace(2, 512, 0), -1);
    assert_eq!(process::sys_trace(usize::MAX, 0, 0), -1);
}
#[test]
fn timeval_uses_seconds_and_microseconds() {
    let mut time = process::TimeVal { sec: 0, usec: 0 };
    assert_eq!(process::sys_get_time(&mut time, 0), 0);
    assert_eq!(time.sec, 1);
    assert_eq!(time.usec, 234_567);
}
