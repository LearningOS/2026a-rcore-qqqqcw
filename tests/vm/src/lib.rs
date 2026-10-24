#![allow(dead_code, unused_imports, unused_variables)]
extern crate alloc;
extern crate self as riscv;
#[macro_use]
extern crate bitflags;
macro_rules! info {
    ($($arg:tt)*) => {};
}
pub mod register {
    pub mod satp {
        pub unsafe fn write(_: usize) {}
    }
}
pub mod config {
    pub const PAGE_SIZE: usize = 4096;
    pub const PAGE_SIZE_BITS: usize = 12;
    pub const KERNEL_STACK_SIZE: usize = 8192;
    pub const USER_STACK_SIZE: usize = 8192;
    pub const MEMORY_END: usize = 0x88000000;
    pub const TRAMPOLINE: usize = usize::MAX - PAGE_SIZE + 1;
    pub const TRAP_CONTEXT_BASE: usize = TRAMPOLINE - PAGE_SIZE;
}
#[path = "../../../os/src/mm/address.rs"]
mod address;
#[path = "../../../os/src/mm/memory_set.rs"]
mod memory_set;
#[path = "../../../os/src/mm/page_table.rs"]
mod page_table;
#[path = "../../../os/src/sync/up.rs"]
pub mod sync;
// The included modules import their sibling types from their parent.
use address::*;
use page_table::*;
#[derive(Debug)]
pub struct FrameTracker {
    pub ppn: PhysPageNum,
}
impl Drop for FrameTracker {
    fn drop(&mut self) {
        unsafe {
            std::alloc::dealloc(
                (self.ppn.0 << 12) as *mut u8,
                std::alloc::Layout::from_size_align(4096, 4096).unwrap(),
            )
        }
    }
}
pub fn frame_alloc() -> Option<FrameTracker> {
    let layout = std::alloc::Layout::from_size_align(4096, 4096).unwrap();
    let ptr = unsafe { std::alloc::alloc_zeroed(layout) };
    if ptr.is_null() {
        None
    } else {
        Some(FrameTracker {
            ppn: PhysPageNum(ptr as usize >> 12),
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use memory_set::MemorySet;
    #[test]
    fn mapping_validation_and_partial_unmapping() {
        let mut memory = MemorySet::new_bare();
        assert_eq!(memory.mmap(0x10001, 4096, 3), -1);
        assert_eq!(memory.mmap(0x10000, 4096, 0), -1);
        assert_eq!(memory.mmap(0x10000, 4096, 8), -1);
        assert_eq!(memory.mmap(usize::MAX - 4095, 8192, 3), -1);
        assert_eq!(memory.mmap(1 << 38, 4096, 3), -1);
        assert_eq!(memory.mmap(0x10000, 3 * 4096, 3), 0);
        assert_eq!(memory.mmap(0x11000, 4096, 3), -1);
        assert_eq!(memory.munmap(0x12000, 2 * 4096), -1);
        assert!(memory.translate(VirtAddr::from(0x12000).floor()).is_some());
        assert_eq!(memory.munmap(0x11000, 1), 0);
        assert!(memory.translate(VirtAddr::from(0x11000).floor()).is_none());
        assert!(memory.translate(VirtAddr::from(0x10000).floor()).is_some());
        assert!(memory.translate(VirtAddr::from(0x12000).floor()).is_some());
        assert_eq!(memory.munmap(0x11000, 1), -1);
        assert_eq!(memory.mmap(0x11000, 4096, 3), 0);
        assert_eq!(memory.munmap(0x10000, 3 * 4096), 0);
        assert_eq!(memory.mmap(0x10000, 3 * 4096 - 1, 3), 0);
    }
    #[test]
    fn checked_copy_spans_noncontiguous_pages_and_checks_permissions() {
        let mut memory = MemorySet::new_bare();
        assert_eq!(memory.mmap(0x20000, 2 * 4096, 3), 0);
        let token = memory.token();
        assert!(copy_to_user(token, 0x20ff8 as *mut u8, &[0xa5; 16]));
        let bytes: Vec<u8> = checked_user_buffer(token, 0x20ff8 as *const u8, 16, false)
            .unwrap()
            .iter()
            .flat_map(|part| part.iter().copied())
            .collect();
        assert_eq!(bytes, vec![0xa5; 16]);
        assert!(!copy_to_user(token, 0x21ff8 as *mut u8, &[0xff; 16]));
        let before = checked_user_buffer(token, 0x21ff8 as *const u8, 8, false).unwrap();
        assert_eq!(before[0], [0; 8]);
        assert_eq!(memory.mmap(0x30000, 4096, 1), 0);
        assert!(!copy_to_user(token, 0x30000 as *mut u8, &[1]));
        assert_eq!(memory.mmap(0x40000, 4096, 4), 0);
        assert!(checked_user_buffer(token, 0x40000 as *const u8, 1, false).is_none());
        memory.insert_framed_area(0x50000.into(), 0x51000.into(), memory_set::MapPermission::R);
        assert!(checked_user_buffer(token, 0x50000 as *const u8, 1, false).is_none());
        assert!(checked_user_buffer(token, usize::MAX as *const u8, 2, true).is_none());
        assert_eq!(memory.munmap(0x50000, 4096), -1);
    }
}

// Exercise the real ready-queue implementation with small mock task records.
pub struct TaskControlBlock {
    inner: sync::UPSafeCell<SchedulingState>,
}
pub struct SchedulingState {
    priority: usize,
    stride: u128,
}
impl TaskControlBlock {
    pub fn inner_exclusive_access(&self) -> std::cell::RefMut<'_, SchedulingState> {
        self.inner.exclusive_access()
    }
}
#[path = "../../../os/src/task/manager.rs"]
mod manager;
#[cfg(test)]
mod scheduling_tests {
    use super::*;
    use std::sync::Arc;
    fn task(priority: usize) -> Arc<TaskControlBlock> {
        Arc::new(TaskControlBlock {
            inner: unsafe {
                sync::UPSafeCell::new(SchedulingState {
                    priority,
                    stride: 0,
                })
            },
        })
    }
    #[test]
    fn weights_produce_proportional_dispatch() {
        let tasks = [task(2), task(4), task(8)];
        let mut scheduler = manager::TaskManager::new();
        assert!(scheduler.fetch().is_none());
        for task in &tasks {
            scheduler.add(task.clone());
        }
        let mut counts = [0; 3];
        for _ in 0..700 {
            let selected = scheduler.fetch().unwrap();
            let i = tasks
                .iter()
                .position(|task| Arc::ptr_eq(task, &selected))
                .unwrap();
            counts[i] += 1;
            scheduler.add(selected);
        }
        assert_eq!(counts, [100, 200, 400]);
    }
    #[test]
    fn equal_weights_rotate_and_large_priority_still_advances() {
        let first = task(16);
        let second = task(16);
        let mut scheduler = manager::TaskManager::new();
        scheduler.add(first.clone());
        scheduler.add(second.clone());
        for expected in [&first, &second, &first, &second] {
            let selected = scheduler.fetch().unwrap();
            assert!(Arc::ptr_eq(expected, &selected));
            scheduler.add(selected);
        }
        let huge = task(isize::MAX as usize);
        let mut scheduler = manager::TaskManager::new();
        scheduler.add(huge.clone());
        scheduler.fetch().unwrap();
        assert_eq!(huge.inner_exclusive_access().stride, 1);
    }
}
