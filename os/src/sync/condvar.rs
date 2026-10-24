//! Conditian variable

use crate::sync::{Mutex, ResourceKind, UPSafeCell};
use crate::task::{
    block_current_and_run_next, current_process, current_task, wakeup_task, TaskControlBlock,
};
use alloc::{collections::VecDeque, sync::Arc};

/// Condition variable structure
pub struct Condvar {
    /// Condition variable inner
    pub inner: UPSafeCell<CondvarInner>,
}

pub struct CondvarInner {
    pub wait_queue: VecDeque<Arc<TaskControlBlock>>,
}

impl Condvar {
    /// Create a new condition variable
    pub fn new() -> Self {
        trace!("kernel: Condvar::new");
        Self {
            inner: unsafe {
                UPSafeCell::new(CondvarInner {
                    wait_queue: VecDeque::new(),
                })
            },
        }
    }

    /// Signal a task waiting on the condition variable
    pub fn signal(&self) {
        let mut inner = self.inner.exclusive_access();
        if let Some(task) = inner.wait_queue.pop_front() {
            wakeup_task(task);
        }
    }

    /// blocking current task, let it wait on the condition variable
    pub fn wait(&self, mutex: Arc<dyn Mutex>, mutex_id: usize) -> isize {
        let process = current_process();
        let tid = current_task()
            .unwrap()
            .inner_exclusive_access()
            .res
            .as_ref()
            .unwrap()
            .tid;
        if !process
            .inner_exclusive_access()
            .deadlock
            .release(tid, ResourceKind::Mutex, mutex_id)
        {
            return -1;
        }
        trace!("kernel: Condvar::wait_with_mutex");
        mutex.unlock();
        let mut inner = self.inner.exclusive_access();
        inner.wait_queue.push_back(current_task().unwrap());
        drop(inner);
        block_current_and_run_next();
        if !process
            .inner_exclusive_access()
            .deadlock
            .request(tid, ResourceKind::Mutex, mutex_id)
        {
            return -0xdead;
        }
        mutex.lock();
        process
            .inner_exclusive_access()
            .deadlock
            .acquired(tid, ResourceKind::Mutex, mutex_id);
        0
    }
}
