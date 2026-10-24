//! Synchronization syscalls with transactional resource-request accounting.
use crate::sync::{Condvar, Mutex, MutexBlocking, MutexSpin, ResourceKind, Semaphore};
use crate::task::{block_current_and_run_next, current_process, current_task};
use crate::timer::{add_timer, get_time_ms};
use alloc::sync::Arc;

fn current_tid() -> usize {
    current_task()
        .unwrap()
        .inner_exclusive_access()
        .res
        .as_ref()
        .unwrap()
        .tid
}

/// Sleep the current thread until its timer expires.
pub fn sys_sleep(ms: usize) -> isize {
    let expire_ms = get_time_ms().saturating_add(ms);
    add_timer(expire_ms, current_task().unwrap());
    block_current_and_run_next();
    0
}

/// Create a mutex and register its single resource instance.
pub fn sys_mutex_create(blocking: bool) -> isize {
    let process = current_process();
    let mutex: Arc<dyn Mutex> = if blocking {
        Arc::new(MutexBlocking::new())
    } else {
        Arc::new(MutexSpin::new())
    };
    let mut inner = process.inner_exclusive_access();
    let id = inner
        .mutex_list
        .iter()
        .position(Option::is_none)
        .unwrap_or(inner.mutex_list.len());
    if id == inner.mutex_list.len() {
        inner.mutex_list.push(Some(mutex));
    } else {
        inner.mutex_list[id] = Some(mutex);
    }
    inner.deadlock.add_resource(ResourceKind::Mutex, id, 1);
    id as isize
}

/// Reject deadlocking requests before touching the mutex's wait queue.
pub fn sys_mutex_lock(id: usize) -> isize {
    let process = current_process();
    let tid = current_tid();
    let mut inner = process.inner_exclusive_access();
    let mutex = match inner.mutex_list.get(id).and_then(|mutex| mutex.as_ref()) {
        Some(mutex) => mutex.clone(),
        None => return -1,
    };
    if !inner.deadlock.request(tid, ResourceKind::Mutex, id) {
        return -0xdead;
    }
    drop(inner);
    mutex.lock();
    process
        .inner_exclusive_access()
        .deadlock
        .acquired(tid, ResourceKind::Mutex, id);
    0
}

/// Release an owned mutex and update resource availability before waking waiters.
pub fn sys_mutex_unlock(id: usize) -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let mutex = match inner.mutex_list.get(id).and_then(|mutex| mutex.as_ref()) {
        Some(mutex) => mutex.clone(),
        None => return -1,
    };
    if !inner
        .deadlock
        .release(current_tid(), ResourceKind::Mutex, id)
    {
        return -1;
    }
    drop(inner);
    mutex.unlock();
    0
}

/// Create a semaphore and register its initial available count.
pub fn sys_semaphore_create(count: usize) -> isize {
    if count > isize::MAX as usize {
        return -1;
    }
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let id = inner
        .semaphore_list
        .iter()
        .position(Option::is_none)
        .unwrap_or(inner.semaphore_list.len());
    let sem = Some(Arc::new(Semaphore::new(count)));
    if id == inner.semaphore_list.len() {
        inner.semaphore_list.push(sem);
    } else {
        inner.semaphore_list[id] = sem;
    }
    inner
        .deadlock
        .add_resource(ResourceKind::Semaphore, id, count);
    id as isize
}

/// Signal a semaphore, including signals from threads that did not acquire it.
pub fn sys_semaphore_up(id: usize) -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let sem = match inner.semaphore_list.get(id).and_then(|sem| sem.as_ref()) {
        Some(sem) => sem.clone(),
        None => return -1,
    };
    inner
        .deadlock
        .release(current_tid(), ResourceKind::Semaphore, id);
    drop(inner);
    sem.up();
    0
}

/// Request a semaphore instance, rolling back a detected deadlock.
pub fn sys_semaphore_down(id: usize) -> isize {
    let process = current_process();
    let tid = current_tid();
    let mut inner = process.inner_exclusive_access();
    let sem = match inner.semaphore_list.get(id).and_then(|sem| sem.as_ref()) {
        Some(sem) => sem.clone(),
        None => return -1,
    };
    if !inner.deadlock.request(tid, ResourceKind::Semaphore, id) {
        return -0xdead;
    }
    drop(inner);
    sem.down();
    process
        .inner_exclusive_access()
        .deadlock
        .acquired(tid, ResourceKind::Semaphore, id);
    0
}

/// Allocate a condition variable.
pub fn sys_condvar_create() -> isize {
    let process = current_process();
    let mut inner = process.inner_exclusive_access();
    let id = inner
        .condvar_list
        .iter()
        .position(Option::is_none)
        .unwrap_or(inner.condvar_list.len());
    let condvar = Some(Arc::new(Condvar::new()));
    if id == inner.condvar_list.len() {
        inner.condvar_list.push(condvar);
    } else {
        inner.condvar_list[id] = condvar;
    }
    id as isize
}

/// Wake one condition-variable waiter.
pub fn sys_condvar_signal(id: usize) -> isize {
    let process = current_process();
    let inner = process.inner_exclusive_access();
    let condvar = match inner
        .condvar_list
        .get(id)
        .and_then(|condvar| condvar.as_ref())
    {
        Some(condvar) => condvar.clone(),
        None => return -1,
    };
    drop(inner);
    condvar.signal();
    0
}

/// Wait on a condition variable, tracking the released and reacquired mutex.
pub fn sys_condvar_wait(id: usize, mutex_id: usize) -> isize {
    let process = current_process();
    let inner = process.inner_exclusive_access();
    let condvar = match inner
        .condvar_list
        .get(id)
        .and_then(|condvar| condvar.as_ref())
    {
        Some(condvar) => condvar.clone(),
        None => return -1,
    };
    let mutex = match inner
        .mutex_list
        .get(mutex_id)
        .and_then(|mutex| mutex.as_ref())
    {
        Some(mutex) => mutex.clone(),
        None => return -1,
    };
    drop(inner);
    condvar.wait(mutex, mutex_id)
}

/// Toggle deadlock detection; accounting remains correct while detection is disabled.
pub fn sys_enable_deadlock_detect(enabled: usize) -> isize {
    if enabled > 1 {
        return -1;
    }
    current_process().inner_exclusive_access().deadlock.enabled = enabled == 1;
    0
}
