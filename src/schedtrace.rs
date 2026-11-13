//! Scheduler trace (CA-6 layers 5 and 6): a ring of the last context switches, each with the tick it
//! happened at, which task left, which task came in, and WHY the task left. The reason is what lets
//! cooperative switching (a task gives the CPU up: `Yield`, `Block`, `Sleep`, `Exit`) be told apart
//! from timer-driven preemption (`Timer`). Pure and host-testable; the kernel keeps one ring.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// The timer interrupt took the CPU away from a task that was still running.
    Timer,
    /// The task called `yield_now`.
    Yield,
    /// The task blocked on a wait queue.
    Block,
    /// The task went to sleep.
    Sleep,
    /// The task ended (or was killed).
    Exit,
}

impl Reason {
    pub const COUNT: usize = 5;
    pub const ALL: [Reason; Reason::COUNT] = [Reason::Timer, Reason::Yield, Reason::Block, Reason::Sleep, Reason::Exit];

    pub fn name(self) -> &'static str {
        match self {
            Reason::Timer => "timer",
            Reason::Yield => "yield",
            Reason::Block => "block",
            Reason::Sleep => "sleep",
            Reason::Exit => "exit",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// Timer ticks since boot when the switch happened (100 per second).
    pub tick: u64,
    pub from: u8,
    pub to: u8,
    pub reason: Reason,
}

const EMPTY: Event = Event { tick: 0, from: 0, to: 0, reason: Reason::Timer };

#[derive(Clone, Copy)]
pub struct TraceRing<const N: usize> {
    events: [Event; N],
    /// Index the next event is written at.
    next: usize,
    total: u64,
    by_reason: [u64; Reason::COUNT],
}

impl<const N: usize> TraceRing<N> {
    pub const fn new() -> Self {
        Self { events: [EMPTY; N], next: 0, total: 0, by_reason: [0; Reason::COUNT] }
    }

    /// Records one switch. The oldest event is overwritten when the ring is full; the totals keep counting.
    pub fn push(&mut self, e: Event) {
        self.events[self.next] = e;
        self.next = (self.next + 1) % N;
        self.total += 1;
        self.by_reason[e.reason.index()] += 1;
    }

    /// Switches recorded since the last `reset` (including ones that have already been overwritten).
    pub fn total(&self) -> u64 {
        self.total
    }

    pub fn count(&self, r: Reason) -> u64 {
        self.by_reason[r.index()]
    }

    /// How many events the ring still holds.
    pub fn len(&self) -> usize {
        self.total.min(N as u64) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    /// The events still held, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = Event> + '_ {
        let len = self.len();
        let start = if self.total as usize >= N { self.next } else { 0 };
        (0..len).map(move |i| self.events[(start + i) % N])
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl<const N: usize> Default for TraceRing<N> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn ev(tick: u64, from: u8, to: u8, reason: Reason) -> Event {
        Event { tick, from, to, reason }
    }

    #[test]
    fn records_in_order_and_counts_by_reason() {
        let mut t = TraceRing::<8>::new();
        assert!(t.is_empty());
        t.push(ev(1, 1, 2, Reason::Timer));
        t.push(ev(2, 2, 3, Reason::Yield));
        t.push(ev(3, 3, 1, Reason::Timer));
        assert_eq!(t.total(), 3);
        assert_eq!(t.count(Reason::Timer), 2);
        assert_eq!(t.count(Reason::Yield), 1);
        assert_eq!(t.count(Reason::Block), 0);
        let v: Vec<_> = t.iter().map(|e| e.tick).collect();
        assert_eq!(v, [1, 2, 3]);
    }

    #[test]
    fn the_oldest_events_are_overwritten_but_totals_keep_counting() {
        let mut t = TraceRing::<4>::new();
        for i in 0..10u64 {
            t.push(ev(i, 0, 1, if i % 2 == 0 { Reason::Timer } else { Reason::Sleep }));
        }
        assert_eq!(t.total(), 10);
        assert_eq!(t.len(), 4);
        let v: Vec<_> = t.iter().map(|e| e.tick).collect();
        assert_eq!(v, [6, 7, 8, 9], "the last four, oldest first");
        assert_eq!(t.count(Reason::Timer), 5);
        assert_eq!(t.count(Reason::Sleep), 5);
    }

    #[test]
    fn exactly_full_is_in_order() {
        let mut t = TraceRing::<3>::new();
        for i in 0..3u64 {
            t.push(ev(i, 0, 0, Reason::Exit));
        }
        assert_eq!(t.iter().map(|e| e.tick).collect::<Vec<_>>(), [0, 1, 2]);
    }

    #[test]
    fn reset_clears_everything() {
        let mut t = TraceRing::<4>::new();
        t.push(ev(1, 1, 2, Reason::Block));
        t.reset();
        assert!(t.is_empty());
        assert_eq!(t.total(), 0);
        assert_eq!(t.count(Reason::Block), 0);
        assert_eq!(t.iter().count(), 0);
    }

    #[test]
    fn reason_names_are_distinct_and_cover_all() {
        let names: Vec<_> = Reason::ALL.iter().map(|r| r.name()).collect();
        for (i, a) in names.iter().enumerate() {
            for b in &names[i + 1..] {
                assert_ne!(a, b);
            }
        }
        for (i, r) in Reason::ALL.iter().enumerate() {
            assert_eq!(r.index(), i);
        }
    }
}
