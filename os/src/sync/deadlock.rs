//! Per-process detection of cycles among outstanding resource requests.
extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;

/// The two independently numbered synchronization resource namespaces.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ResourceKind {
    /// A mutex has one instance.
    Mutex,
    /// A semaphore may have several instances.
    Semaphore,
}

struct Resource {
    kind: ResourceKind,
    id: usize,
    available: usize,
    allocation: Vec<usize>,
    request: Vec<usize>,
}

/// Allocation and outstanding-request matrices for one process.
pub struct DeadlockDetector {
    /// Detection can be toggled while accounting remains active.
    pub enabled: bool,
    active: Vec<bool>,
    resources: Vec<Resource>,
}

impl DeadlockDetector {
    /// Start with detection disabled and no resources.
    pub fn new() -> Self {
        Self {
            enabled: false,
            active: Vec::new(),
            resources: Vec::new(),
        }
    }

    fn ensure_thread(&mut self, tid: usize) {
        if self.active.len() <= tid {
            self.active.resize(tid + 1, false);
            for resource in &mut self.resources {
                resource.allocation.resize(tid + 1, 0);
                resource.request.resize(tid + 1, 0);
            }
        }
    }

    /// Initialize a newly allocated or recycled thread id.
    pub fn add_thread(&mut self, tid: usize) {
        self.ensure_thread(tid);
        self.active[tid] = true;
        for resource in &mut self.resources {
            // Exiting while holding a resource does not release the actual lock.
            // Its unavailable instances therefore stay unavailable on id reuse.
            resource.allocation[tid] = 0;
            resource.request[tid] = 0;
        }
    }

    /// An exited thread cannot participate in a completion sequence.
    pub fn remove_thread(&mut self, tid: usize) {
        self.ensure_thread(tid);
        self.active[tid] = false;
        for resource in &mut self.resources {
            resource.request[tid] = 0;
        }
    }

    /// Register a newly created mutex or semaphore.
    pub fn add_resource(&mut self, kind: ResourceKind, id: usize, count: usize) {
        self.resources.push(Resource {
            kind,
            id,
            available: count,
            allocation: vec![0; self.active.len()],
            request: vec![0; self.active.len()],
        });
    }

    fn resource_mut(&mut self, kind: ResourceKind, id: usize) -> &mut Resource {
        self.resources
            .iter_mut()
            .find(|resource| resource.kind == kind && resource.id == id)
            .expect("unregistered synchronization resource")
    }

    /// Tentatively request one instance; reject and roll back if no completion sequence exists.
    pub fn request(&mut self, tid: usize, kind: ResourceKind, id: usize) -> bool {
        self.ensure_thread(tid);
        self.active[tid] = true;
        self.resource_mut(kind, id).request[tid] += 1;
        if self.enabled && !self.safe() {
            self.resource_mut(kind, id).request[tid] -= 1;
            false
        } else {
            true
        }
    }

    /// Commit the allocation only after the real synchronization primitive grants it.
    pub fn acquired(&mut self, tid: usize, kind: ResourceKind, id: usize) {
        let resource = self.resource_mut(kind, id);
        assert!(resource.request[tid] > 0 && resource.available > 0);
        resource.request[tid] -= 1;
        resource.available -= 1;
        resource.allocation[tid] += 1;
    }

    /// Release a held instance. A semaphore may also be signaled by a non-holder.
    pub fn release(&mut self, tid: usize, kind: ResourceKind, id: usize) -> bool {
        self.ensure_thread(tid);
        let resource = self.resource_mut(kind, id);
        if resource.allocation[tid] > 0 {
            resource.allocation[tid] -= 1;
        } else if kind == ResourceKind::Mutex {
            return false;
        }
        resource.available += 1;
        true
    }

    fn safe(&self) -> bool {
        let mut work: Vec<usize> = self
            .resources
            .iter()
            .map(|resource| resource.available)
            .collect();
        let mut finished: Vec<bool> = self.active.iter().map(|active| !active).collect();
        loop {
            let next = (0..finished.len()).find(|&tid| {
                !finished[tid]
                    && self
                        .resources
                        .iter()
                        .enumerate()
                        .all(|(r, resource)| resource.request[tid] <= work[r])
            });
            match next {
                Some(tid) => {
                    finished[tid] = true;
                    for (r, resource) in self.resources.iter().enumerate() {
                        work[r] += resource.allocation[tid];
                    }
                }
                None => return finished.into_iter().all(|done| done),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DeadlockDetector, ResourceKind::*};
    #[test]
    fn mutex_cycle_rejected_without_poisoning_future_requests() {
        let mut d = DeadlockDetector::new();
        d.enabled = true;
        d.add_resource(Mutex, 0, 1);
        d.add_resource(Mutex, 1, 1);
        assert!(d.request(0, Mutex, 0));
        d.acquired(0, Mutex, 0);
        assert!(d.request(1, Mutex, 1));
        d.acquired(1, Mutex, 1);
        assert!(d.request(0, Mutex, 1));
        assert!(!d.request(1, Mutex, 0));
        assert!(d.release(1, Mutex, 1));
        d.acquired(0, Mutex, 1);
        assert!(d.release(0, Mutex, 0));
        assert!(d.request(1, Mutex, 0));
        d.acquired(1, Mutex, 0);
    }
    #[test]
    fn self_lock_and_cross_kind_cycles_are_detected() {
        let mut d = DeadlockDetector::new();
        d.enabled = true;
        d.add_resource(Mutex, 0, 1);
        d.add_resource(Semaphore, 0, 1);
        assert!(d.request(0, Mutex, 0));
        d.acquired(0, Mutex, 0);
        assert!(!d.request(0, Mutex, 0));
        assert!(d.request(1, Semaphore, 0));
        d.acquired(1, Semaphore, 0);
        assert!(d.request(0, Semaphore, 0));
        assert!(!d.request(1, Mutex, 0));
    }
    #[test]
    fn multiple_instances_allow_a_completion_sequence() {
        let mut d = DeadlockDetector::new();
        d.enabled = true;
        d.add_resource(Semaphore, 0, 2);
        assert!(d.request(0, Semaphore, 0));
        d.acquired(0, Semaphore, 0);
        assert!(d.request(1, Semaphore, 0));
        d.acquired(1, Semaphore, 0);
        assert!(d.request(0, Semaphore, 0));
        assert!(!d.request(1, Semaphore, 0));
        assert!(d.release(1, Semaphore, 0));
        d.acquired(0, Semaphore, 0);
        assert!(d.release(0, Semaphore, 0));
        assert!(d.release(0, Semaphore, 0));
    }
    #[test]
    fn disabled_tracking_can_be_enabled_later() {
        let mut d = DeadlockDetector::new();
        d.add_resource(Mutex, 0, 1);
        assert!(d.request(9, Mutex, 0));
        d.acquired(9, Mutex, 0);
        d.enabled = true;
        assert!(!d.request(9, Mutex, 0));
        assert!(!d.release(0, Mutex, 0));
        assert!(d.release(9, Mutex, 0));
        assert!(d.request(9, Mutex, 0));
        d.acquired(9, Mutex, 0);
    }
    #[test]
    fn semaphore_signals_and_dynamic_resource_dimensions() {
        let mut d = DeadlockDetector::new();
        d.enabled = true;
        d.add_thread(0);
        d.add_thread(3);
        d.add_resource(Semaphore, 0, 0);
        assert!(d.release(3, Semaphore, 0));
        assert!(d.request(0, Semaphore, 0));
        d.acquired(0, Semaphore, 0);
        d.add_resource(Mutex, 20, 1);
        assert!(d.request(12, Mutex, 20));
        d.acquired(12, Mutex, 20);
        d.remove_thread(12);
        d.add_thread(12);
        assert!(!d.request(12, Mutex, 20));
    }
}
