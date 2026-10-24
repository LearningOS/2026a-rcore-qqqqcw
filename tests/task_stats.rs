#![allow(dead_code)]

mod config {
    pub const MAX_APP_NUM: usize = 16;
}
#[path = "../os/src/sync/up.rs"]
mod sync;
#[path = "../os/src/task/syscall_stats.rs"]
mod syscall_stats;

#[derive(Copy, Clone)]
pub struct TaskContext([usize; 14]);
#[path = "../os/src/task/task.rs"]
mod task;

#[test]
fn counters_are_isolated_and_task_records_fit_on_the_boot_stack() {
    assert!(core::mem::size_of::<task::TaskControlBlock>() * config::MAX_APP_NUM < 8192);
    for id in 0..config::MAX_APP_NUM {
        assert_eq!(syscall_stats::count(id, 169), 0);
        for _ in 0..=id {
            syscall_stats::record(id, 169);
        }
    }
    for id in 0..config::MAX_APP_NUM {
        assert_eq!(syscall_stats::count(id, 169), id as isize + 1);
        assert_eq!(syscall_stats::count(id, 410), 0);
    }
    syscall_stats::record(0, 512);
    assert_eq!(syscall_stats::count(0, 512), -1);
    assert_eq!(syscall_stats::count(16, 169), -1);
}
