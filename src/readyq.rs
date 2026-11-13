//! The ready queue (CA-6 layer 4): a first-in, first-out ring of the ids of tasks that are ready to run.
//! The scheduler takes the task at the front, and puts a task at the back when it becomes ready, so
//! round robin is simply "take from the front, put the preempted task at the back". Pure and
//! host-testable: it holds ids only, not tasks.
//!
//! Rules (the invariants the scheduler relies on): an id is in the queue at most once; the queue never
//! holds more than `N` ids; order is the order of insertion; removing an id keeps the others in order.

#[derive(Clone, Copy, Debug)]
pub struct ReadyQueue<const N: usize> {
    ids: [u8; N],
    /// Index in `ids` of the front element.
    head: usize,
    len: usize,
}

impl<const N: usize> ReadyQueue<N> {
    pub const fn new() -> Self {
        Self { ids: [0; N], head: 0, len: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn contains(&self, id: u8) -> bool {
        self.iter().any(|x| x == id)
    }

    /// Insertion: puts `id` at the back. Returns false (and changes nothing) when the queue is full or
    /// `id` is already in it.
    pub fn push_back(&mut self, id: u8) -> bool {
        if self.len == N || self.contains(id) {
            return false;
        }
        self.ids[(self.head + self.len) % N] = id;
        self.len += 1;
        true
    }

    /// Selection: takes the id at the front, or `None` when the queue is empty.
    pub fn pop_front(&mut self) -> Option<u8> {
        if self.len == 0 {
            return None;
        }
        let id = self.ids[self.head];
        self.head = (self.head + 1) % N;
        self.len -= 1;
        Some(id)
    }

    /// Removal: takes `id` out wherever it is, keeping the others in order. Returns whether it was there.
    pub fn remove(&mut self, id: u8) -> bool {
        let Some(pos) = self.iter().position(|x| x == id) else { return false };
        for i in pos..self.len - 1 {
            self.ids[(self.head + i) % N] = self.ids[(self.head + i + 1) % N];
        }
        self.len -= 1;
        true
    }

    /// Ordering: the ids from the front to the back.
    pub fn iter(&self) -> impl Iterator<Item = u8> + '_ {
        (0..self.len).map(move |i| self.ids[(self.head + i) % N])
    }
}

impl<const N: usize> Default for ReadyQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::collections::VecDeque;
    use alloc::vec::Vec;

    fn order<const N: usize>(q: &ReadyQueue<N>) -> Vec<u8> {
        q.iter().collect()
    }

    #[test]
    fn first_in_first_out() {
        let mut q = ReadyQueue::<4>::new();
        assert!(q.is_empty());
        for id in [3, 1, 2] {
            assert!(q.push_back(id));
        }
        assert_eq!(order(&q), [3, 1, 2]);
        assert_eq!(q.pop_front(), Some(3));
        assert_eq!(q.pop_front(), Some(1));
        assert_eq!(q.pop_front(), Some(2));
        assert_eq!(q.pop_front(), None);
        assert_eq!(q.len(), 0);
    }

    #[test]
    fn round_robin_is_pop_then_push() {
        // The scheduler's whole policy: take the front, run it, put it at the back.
        let mut q = ReadyQueue::<4>::new();
        for id in [1, 2, 3] {
            q.push_back(id);
        }
        let mut seen = Vec::new();
        for _ in 0..7 {
            let id = q.pop_front().unwrap();
            seen.push(id);
            q.push_back(id);
        }
        assert_eq!(seen, [1, 2, 3, 1, 2, 3, 1]);
    }

    #[test]
    fn an_id_is_never_queued_twice() {
        let mut q = ReadyQueue::<4>::new();
        assert!(q.push_back(5));
        assert!(!q.push_back(5), "duplicate refused");
        assert_eq!(order(&q), [5]);
    }

    #[test]
    fn full_queue_refuses_and_keeps_its_content() {
        let mut q = ReadyQueue::<2>::new();
        assert!(q.push_back(1));
        assert!(q.push_back(2));
        assert!(!q.push_back(3));
        assert_eq!(order(&q), [1, 2]);
    }

    #[test]
    fn remove_keeps_the_others_in_order() {
        let mut q = ReadyQueue::<5>::new();
        for id in [1, 2, 3, 4] {
            q.push_back(id);
        }
        assert!(q.remove(2));
        assert_eq!(order(&q), [1, 3, 4]);
        assert!(!q.remove(2), "already gone");
        assert!(q.remove(1), "front");
        assert!(q.remove(4), "back");
        assert_eq!(order(&q), [3]);
        assert!(!q.contains(1) && q.contains(3));
    }

    #[test]
    fn wraps_around_the_ring() {
        let mut q = ReadyQueue::<3>::new();
        for round in 0..10u8 {
            assert!(q.push_back(round));
            if round >= 2 {
                assert_eq!(q.pop_front(), Some(round - 2));
            }
        }
        assert_eq!(order(&q), [8, 9]);
    }

    /// The queue behaves exactly like a plain `VecDeque` (with the no-duplicates rule) under random use.
    #[test]
    fn random_operations_match_a_model() {
        let mut q = ReadyQueue::<8>::new();
        let mut model: VecDeque<u8> = VecDeque::new();
        let mut seed = 99u32;
        let mut next = |m: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) % m
        };
        for _ in 0..5000 {
            let id = next(12) as u8;
            match next(3) {
                0 => {
                    let expect = model.len() < 8 && !model.contains(&id);
                    assert_eq!(q.push_back(id), expect);
                    if expect {
                        model.push_back(id);
                    }
                }
                1 => assert_eq!(q.pop_front(), model.pop_front()),
                _ => {
                    let was = model.iter().position(|&x| x == id);
                    assert_eq!(q.remove(id), was.is_some());
                    if let Some(p) = was {
                        model.remove(p);
                    }
                }
            }
            assert_eq!(order(&q), model.iter().copied().collect::<Vec<_>>());
        }
    }
}
