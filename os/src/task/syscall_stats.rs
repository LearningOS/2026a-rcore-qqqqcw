//! Statically initialized per-task syscall counters, kept off kernel stacks.

use crate::{config::MAX_APP_NUM, sync::UPSafeCell};

const SYSCALL_COUNT: usize = 512;

// The 64 KiB counter table must not be returned by a lazy initializer:
// individual task stacks are only 8 KiB. The kernel runs on one hart and
// handles syscalls with supervisor interrupts disabled, as UPSafeCell requires.
static COUNTS: UPSafeCell<[[usize; SYSCALL_COUNT]; MAX_APP_NUM]> =
    unsafe { UPSafeCell::new([[0; SYSCALL_COUNT]; MAX_APP_NUM]) };

pub(super) fn record(task: usize, syscall: usize) {
    if let Some(count) = COUNTS
        .exclusive_access()
        .get_mut(task)
        .and_then(|counts| counts.get_mut(syscall))
    {
        *count += 1;
    }
}

pub(super) fn count(task: usize, syscall: usize) -> isize {
    COUNTS
        .exclusive_access()
        .get(task)
        .and_then(|counts| counts.get(syscall))
        .map(|count| *count as isize)
        .unwrap_or(-1)
}
