//! Minimal dependency-free user program for a supplemental QEMU smoke test.
#![no_std]
#![no_main]
#![allow(dead_code)]
use core::arch::{asm, global_asm};
#[cfg(ch8)]
use core::sync::atomic::{AtomicUsize, Ordering::SeqCst};
global_asm!(
    ".section .text.entry",
    ".global _start",
    "_start:",
    "call guest_main",
    "1: j 1b"
);
fn call(id: usize, args: [usize; 4]) -> isize {
    let result: isize;
    unsafe {
        asm!("ecall", in("a7") id, inlateout("a0") args[0] => result,
        in("a1") args[1], in("a2") args[2], in("a3") args[3]);
    }
    result
}
fn write(message: &[u8]) {
    call(64, [1, message.as_ptr() as usize, message.len(), 0]);
}
fn exit(code: usize) -> ! {
    call(93, [code, 0, 0, 0]);
    loop {
        core::hint::spin_loop();
    }
}
fn check(ok: bool, label: &[u8]) {
    if !ok {
        write(b"SMOKE FAIL: ");
        write(label);
        write(b"\n");
        exit(1);
    }
}
fn yield_now() {
    call(124, [0; 4]);
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    write(b"SMOKE PANIC\n");
    exit(1)
}
#[no_mangle]
extern "C" fn guest_main() -> ! {
    #[cfg(child)]
    {
        write(b"SMOKE CHILD\n");
        exit(37);
    }
    #[cfg(not(child))]
    {
        #[cfg(ch3)]
        test_trace();
        #[cfg(not(ch3))]
        test_memory();
        #[cfg(any(ch5, ch6, ch8))]
        test_process();
        #[cfg(any(ch6, ch8))]
        test_files();
        #[cfg(ch8)]
        test_sync();
        write(b"SMOKE PASS\n");
        exit(0);
    }
}
#[cfg(ch3)]
fn test_trace() {
    let mut byte = 0x80u8;
    let addr = &mut byte as *mut u8 as usize;
    check(call(410, [0, addr, 0, 0]) == 128, b"trace read");
    check(
        call(410, [1, addr, 0x1ff, 0]) == 0 && byte == 255,
        b"trace write",
    );
    check(call(410, [2, 410, 0, 0]) == 3, b"trace self count");
    let count = call(410, [2, 124, 0, 0]);
    yield_now();
    check(call(410, [2, 124, 0, 0]) == count + 1, b"yield count");
    check(call(410, [99, 0, 0, 0]) == -1, b"trace bad request");
}
#[cfg(not(ch3))]
fn test_memory() {
    const START: usize = 0x1000_0000;
    check(call(222, [START, 3 * 4096, 3, 0]) == 0, b"mmap");
    check(call(222, [START, 4096, 3, 0]) == -1, b"mmap overlap");
    check(call(222, [START + 1, 4096, 3, 0]) == -1, b"mmap alignment");
    check(
        call(169, [START + 4096 - 8, 0, 0, 0]) == 0,
        b"gettime cross page",
    );
    check(
        unsafe { ((START + 4096) as *const usize).read_volatile() } < 1_000_000,
        b"time microseconds",
    );
    check(
        call(169, [START + 3 * 4096 - 8, 0, 0, 0]) == -1,
        b"gettime missing page",
    );
    check(call(215, [START + 4096, 4096, 0, 0]) == 0, b"munmap middle");
    check(
        call(215, [START, 3 * 4096, 0, 0]) == -1,
        b"munmap atomic rejection",
    );
    check(
        call(222, [START + 4096, 4096, 1, 0]) == 0,
        b"remap read only",
    );
    check(
        call(169, [START + 4096, 0, 0, 0]) == -1,
        b"gettime read only",
    );
    check(call(215, [START, 3 * 4096, 0, 0]) == 0, b"munmap all");
    #[cfg(ch4)]
    {
        check(call(410, [0, START, 0, 0]) == -1, b"trace unmapped");
        check(
            call(410, [0, 0xffff_ffff_ffff_e000, 0, 0]) == -1,
            b"trace kernel",
        );
    }
}
#[cfg(any(ch5, ch6, ch8))]
fn test_process() {
    check(call(140, [1, 0, 0, 0]) == -1, b"priority invalid");
    check(call(140, [16, 0, 0, 0]) == 16, b"priority valid");
    let pid = call(400, [b"child\0".as_ptr() as usize, 0, 0, 0]);
    check(pid > 0, b"spawn");
    let mut code = 0i32;
    loop {
        let result = call(260, [pid as usize, &mut code as *mut i32 as usize, 0, 0]);
        if result == -2 {
            yield_now();
            continue;
        }
        check(result == pid && code == 37, b"spawn waitpid");
        break;
    }
    check(
        call(400, [b"not-a-program\0".as_ptr() as usize, 0, 0, 0]) == -1,
        b"spawn missing",
    );
    let pid = call(220, [0; 4]);
    check(pid >= 0, b"fork");
    if pid == 0 {
        exit(19);
    }
    loop {
        let result = call(260, [pid as usize, &mut code as *mut i32 as usize, 0, 0]);
        if result == -2 {
            yield_now();
            continue;
        }
        check(result == pid && code == 19, b"fork waitpid");
        break;
    }
}
#[cfg(any(ch6, ch8))]
fn test_files() {
    let original = b"smoke-file\0";
    let alias = b"smoke-alias\0";
    let fd = call(56, [0, original.as_ptr() as usize, 514, 0]);
    check(fd >= 3, b"open create");
    check(
        call(64, [fd as usize, b"hello".as_ptr() as usize, 5, 0]) == 5,
        b"file write",
    );
    check(
        call(
            37,
            [0, original.as_ptr() as usize, 0, alias.as_ptr() as usize],
        ) == 0,
        b"link",
    );
    const BUFFER: usize = 0x2000_0000;
    check(call(222, [BUFFER, 8192, 3, 0]) == 0, b"stat memory");
    let stat = BUFFER + 4096 - 8;
    check(
        call(80, [fd as usize, stat, 0, 0]) == 0,
        b"fstat cross page",
    );
    let ino = unsafe { ((stat + 8) as *const u64).read_volatile() };
    check(
        unsafe { ((stat + 20) as *const u32).read_volatile() } == 2,
        b"nlink two",
    );
    check(call(57, [fd as usize, 0, 0, 0]) == 0, b"close");
    check(call(80, [fd as usize, stat, 0, 0]) == -1, b"closed fstat");
    let fd = call(56, [0, alias.as_ptr() as usize, 0, 0]);
    check(fd >= 3, b"open alias");
    check(
        call(35, [0, original.as_ptr() as usize, 0, 0]) == 0,
        b"unlink original",
    );
    check(
        call(35, [0, alias.as_ptr() as usize, 0, 0]) == 0,
        b"unlink alias",
    );
    check(
        call(80, [fd as usize, stat, 0, 0]) == 0,
        b"open unlinked fstat",
    );
    check(
        unsafe { ((stat + 8) as *const u64).read_volatile() } == ino,
        b"inode stable",
    );
    check(
        unsafe { ((stat + 20) as *const u32).read_volatile() } == 0,
        b"nlink zero",
    );
    check(
        call(63, [fd as usize, BUFFER, 5, 0]) == 5,
        b"open unlinked read",
    );
    check(
        unsafe { core::slice::from_raw_parts(BUFFER as *const u8, 5) } == b"hello",
        b"shared content",
    );
    check(call(57, [fd as usize, 0, 0, 0]) == 0, b"final close");
    check(call(215, [BUFFER, 8192, 0, 0]) == 0, b"stat munmap");
}
#[cfg(ch8)]
static FIRST: AtomicUsize = AtomicUsize::new(0);
#[cfg(ch8)]
static SECOND: AtomicUsize = AtomicUsize::new(0);
#[cfg(ch8)]
static BARRIER: AtomicUsize = AtomicUsize::new(0);
#[cfg(ch8)]
static REJECTED: AtomicUsize = AtomicUsize::new(0);
#[cfg(ch8)]
static SEMAPHORE_MODE: AtomicUsize = AtomicUsize::new(0);
#[cfg(ch8)]
extern "C" fn cycle_thread(index: usize) -> ! {
    let sem = SEMAPHORE_MODE.load(SeqCst) != 0;
    let take = if sem { 470 } else { 464 };
    let release = if sem { 468 } else { 466 };
    let a = FIRST.load(SeqCst);
    let b = SECOND.load(SeqCst);
    let (own, other) = if index == 0 { (a, b) } else { (b, a) };
    check(call(take, [own, 0, 0, 0]) == 0, b"take own");
    BARRIER.fetch_add(1, SeqCst);
    while BARRIER.load(SeqCst) < 2 {
        yield_now();
    }
    let result = call(take, [other, 0, 0, 0]);
    if result == -0xdead {
        REJECTED.fetch_add(1, SeqCst);
    } else {
        check(result == 0, b"cycle result");
        check(call(release, [other, 0, 0, 0]) == 0, b"release other");
    }
    check(call(release, [own, 0, 0, 0]) == 0, b"release own");
    exit(0)
}
#[cfg(ch8)]
fn join(tid: isize) {
    check(tid > 0, b"thread create");
    loop {
        let result = call(462, [tid as usize, 0, 0, 0]);
        if result == -2 {
            yield_now();
            continue;
        }
        check(result == 0, b"thread join");
        break;
    }
}
#[cfg(ch8)]
extern "C" fn condition_thread(_: usize) -> ! {
    let mutex = FIRST.load(SeqCst);
    let condvar = SECOND.load(SeqCst);
    check(call(464, [mutex, 0, 0, 0]) == 0, b"condition lock");
    BARRIER.store(1, SeqCst);
    check(call(473, [condvar, mutex, 0, 0]) == 0, b"condition wait");
    check(BARRIER.load(SeqCst) == 2, b"condition wake predicate");
    check(
        call(466, [mutex, 0, 0, 0]) == 0,
        b"condition reacquired mutex",
    );
    exit(0)
}
#[cfg(ch8)]
fn test_sync() {
    check(call(469, [2, 0, 0, 0]) == -1, b"detect invalid");
    check(call(469, [1, 0, 0, 0]) == 0, b"detect enable");
    let mutex = call(463, [1, 0, 0, 0]) as usize;
    check(call(464, [mutex, 0, 0, 0]) == 0, b"mutex lock");
    check(call(464, [mutex, 0, 0, 0]) == -0xdead, b"self deadlock");
    check(call(466, [mutex, 0, 0, 0]) == 0, b"mutex unlock");
    for sem in 0..2 {
        SEMAPHORE_MODE.store(sem, SeqCst);
        BARRIER.store(0, SeqCst);
        REJECTED.store(0, SeqCst);
        let create = if sem != 0 { 467 } else { 463 };
        FIRST.store(call(create, [1, 0, 0, 0]) as usize, SeqCst);
        SECOND.store(call(create, [1, 0, 0, 0]) as usize, SeqCst);
        let first = call(460, [cycle_thread as usize, 0, 0, 0]);
        let second = call(460, [cycle_thread as usize, 1, 0, 0]);
        join(first);
        join(second);
        check(REJECTED.load(SeqCst) == 1, b"one cycle rejection");
    }
    FIRST.store(call(463, [1, 0, 0, 0]) as usize, SeqCst);
    SECOND.store(call(471, [0; 4]) as usize, SeqCst);
    BARRIER.store(0, SeqCst);
    let waiter = call(460, [condition_thread as usize, 0, 0, 0]);
    while BARRIER.load(SeqCst) != 1 {
        yield_now();
    }
    check(
        call(464, [FIRST.load(SeqCst), 0, 0, 0]) == 0,
        b"condition main lock",
    );
    BARRIER.store(2, SeqCst);
    check(
        call(472, [SECOND.load(SeqCst), 0, 0, 0]) == 0,
        b"condition signal",
    );
    check(
        call(466, [FIRST.load(SeqCst), 0, 0, 0]) == 0,
        b"condition main unlock",
    );
    join(waiter);
    check(call(462, [usize::MAX, 0, 0, 0]) == -1, b"invalid waittid");
}
