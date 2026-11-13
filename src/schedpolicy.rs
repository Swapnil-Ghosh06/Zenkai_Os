//! CPU scheduling policies as pure functions: give it processes (arrival, burst, priority) and a policy,
//! get back the Gantt chart and each process's completion time. No kernel state, so it runs on the host.
//! The test cases are the worked examples of the OS course notes (Chapter 5): the same four processes
//! under FCFS (average wait 8.75), SJF (7.75), SRTF (6.5), round robin with quantum 4 (11.75) and
//! preemptive priority (7.0; the notes print 7.75 because that example has an arithmetic slip, see the
//! test), so the code is checked against numbers a person computed by hand.
//!
//! Time is whole ticks. Ties are broken by earlier arrival, then by lower index, so results are stable.

// Index loops read more like the textbook algorithms (`for each process i ...`) than iterator chains.
#![allow(clippy::needless_range_loop)]

use alloc::collections::VecDeque;
use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    pub arrival: u32,
    /// CPU time needed, in ticks.
    pub burst: u32,
    /// Smaller number = more important (only the priority policies read it).
    pub priority: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Policy {
    /// First come, first served.
    Fcfs,
    /// Shortest job first, never interrupting a running job.
    Sjf,
    /// Shortest remaining time first (SJF that may preempt).
    Srtf,
    /// Most important process first; `preemptive` lets a newcomer take the CPU.
    Priority { preemptive: bool },
    /// Round robin with a time slice of `quantum` ticks (0 is treated as 1).
    RoundRobin { quantum: u32 },
}

/// One stretch of CPU time: process `proc` ran from `start` to `end` (end not included).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slice {
    pub proc: usize,
    pub start: u32,
    pub end: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Schedule {
    procs: Vec<Proc>,
    pub gantt: Vec<Slice>,
    completion: Vec<u32>,
    first_run: Vec<u32>,
}

impl Schedule {
    pub fn completion(&self, p: usize) -> u32 {
        self.completion[p]
    }

    /// Completion minus arrival.
    pub fn turnaround(&self, p: usize) -> u32 {
        self.completion[p] - self.procs[p].arrival
    }

    /// Turnaround minus burst: the time spent ready but not running.
    pub fn waiting(&self, p: usize) -> u32 {
        self.turnaround(p) - self.procs[p].burst
    }

    /// First time on the CPU minus arrival.
    pub fn response(&self, p: usize) -> u32 {
        self.first_run[p] - self.procs[p].arrival
    }

    pub fn total_waiting(&self) -> u64 {
        (0..self.procs.len()).map(|p| u64::from(self.waiting(p))).sum()
    }

    pub fn total_turnaround(&self) -> u64 {
        (0..self.procs.len()).map(|p| u64::from(self.turnaround(p))).sum()
    }

    pub fn total_response(&self) -> u64 {
        (0..self.procs.len()).map(|p| u64::from(self.response(p))).sum()
    }

    pub fn len(&self) -> usize {
        self.procs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.procs.is_empty()
    }
}

/// Appends a run of `p` from `start` to `end`, joining it to the previous slice when they touch.
fn push_slice(g: &mut Vec<Slice>, p: usize, start: u32, end: u32) {
    if end <= start {
        return;
    }
    if let Some(last) = g.last_mut() {
        if last.proc == p && last.end == start {
            last.end = end;
            return;
        }
    }
    g.push(Slice { proc: p, start, end });
}

/// Runs the policy and returns the schedule. Processes with a burst of 0 finish the moment they arrive.
pub fn simulate(procs: &[Proc], policy: Policy) -> Schedule {
    let n = procs.len();
    let mut s = Schedule { procs: procs.to_vec(), gantt: Vec::new(), completion: vec![0; n], first_run: vec![0; n] };
    let mut rem: Vec<u32> = procs.iter().map(|p| p.burst).collect();
    let mut done = vec![false; n];
    let mut started = vec![false; n];
    for p in 0..n {
        if rem[p] == 0 {
            done[p] = true;
            s.completion[p] = procs[p].arrival;
            s.first_run[p] = procs[p].arrival;
            started[p] = true;
        }
    }
    let mut t = 0u32;
    let next_arrival = |t: u32, done: &[bool]| (0..n).filter(|&p| !done[p] && procs[p].arrival > t).map(|p| procs[p].arrival).min();
    // Arrived and unfinished, best first according to `key`.
    let pick = |t: u32, done: &[bool], key: &dyn Fn(usize) -> (u32, u32, usize)| (0..n).filter(|&p| !done[p] && procs[p].arrival <= t).min_by_key(|&p| key(p));

    if let Policy::RoundRobin { quantum } = policy {
        let q = quantum.max(1);
        let mut queue: VecDeque<usize> = VecDeque::new();
        let mut queued = vec![false; n];
        // Newcomers join the queue in arrival order, ahead of a process that was just preempted.
        let admit = |t: u32, queue: &mut VecDeque<usize>, queued: &mut [bool], done: &[bool]| {
            let mut arrived: Vec<usize> = (0..n).filter(|&p| !queued[p] && !done[p] && procs[p].arrival <= t).collect();
            arrived.sort_by_key(|&p| (procs[p].arrival, p));
            for p in arrived {
                queued[p] = true;
                queue.push_back(p);
            }
        };
        loop {
            admit(t, &mut queue, &mut queued, &done);
            let Some(p) = queue.pop_front() else {
                match next_arrival(t, &done) {
                    Some(a) => {
                        t = a;
                        continue;
                    }
                    None => break,
                }
            };
            if !started[p] {
                started[p] = true;
                s.first_run[p] = t;
            }
            let run = q.min(rem[p]);
            push_slice(&mut s.gantt, p, t, t + run);
            t += run;
            rem[p] -= run;
            admit(t, &mut queue, &mut queued, &done);
            if rem[p] == 0 {
                done[p] = true;
                s.completion[p] = t;
            } else {
                queue.push_back(p);
            }
        }
        return s;
    }

    loop {
        let key = |p: usize| -> (u32, u32, usize) {
            let pr = &procs[p];
            match policy {
                Policy::Fcfs => (pr.arrival, 0, p),
                Policy::Sjf => (pr.burst, pr.arrival, p),
                Policy::Srtf => (rem[p], pr.arrival, p),
                Policy::Priority { .. } => (pr.priority, pr.arrival, p),
                Policy::RoundRobin { .. } => (0, 0, p),
            }
        };
        let Some(p) = pick(t, &done, &key) else {
            match next_arrival(t, &done) {
                Some(a) => {
                    t = a;
                    continue;
                }
                None => break,
            }
        };
        if !started[p] {
            started[p] = true;
            s.first_run[p] = t;
        }
        let preemptive = matches!(policy, Policy::Srtf | Policy::Priority { preemptive: true });
        // A preemptive policy decides again at every tick; the others run the job to the end.
        let run = if preemptive { 1 } else { rem[p] };
        push_slice(&mut s.gantt, p, t, t + run);
        t += run;
        rem[p] -= run;
        if rem[p] == 0 {
            done[p] = true;
            s.completion[p] = t;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The course's four processes: P1 (0, 8), P2 (1, 4), P3 (2, 9), P4 (3, 5), priorities 2, 1, 4, 3.
    fn course() -> Vec<Proc> {
        vec![Proc { arrival: 0, burst: 8, priority: 2 }, Proc { arrival: 1, burst: 4, priority: 1 }, Proc { arrival: 2, burst: 9, priority: 4 }, Proc { arrival: 3, burst: 5, priority: 3 }]
    }

    fn waits(s: &Schedule) -> Vec<u32> {
        (0..s.len()).map(|p| s.waiting(p)).collect()
    }

    fn sl(proc: usize, start: u32, end: u32) -> Slice {
        Slice { proc, start, end }
    }

    #[test]
    fn fcfs_average_wait_is_8_75() {
        let s = simulate(&course(), Policy::Fcfs);
        assert_eq!(s.gantt, [sl(0, 0, 8), sl(1, 8, 12), sl(2, 12, 21), sl(3, 21, 26)]);
        assert_eq!(waits(&s), [0, 7, 10, 18]);
        assert_eq!(s.total_waiting(), 35, "35 / 4 = 8.75");
        assert_eq!(s.total_turnaround(), 61, "61 / 4 = 15.25");
        // The notes' cross-check: sum turnaround = sum waiting + sum bursts.
        assert_eq!(s.total_turnaround(), s.total_waiting() + 26);
    }

    #[test]
    fn sjf_average_wait_is_7_75() {
        let s = simulate(&course(), Policy::Sjf);
        assert_eq!(s.gantt, [sl(0, 0, 8), sl(1, 8, 12), sl(3, 12, 17), sl(2, 17, 26)]);
        assert_eq!(s.total_waiting(), 31, "31 / 4 = 7.75");
        assert_eq!(s.total_turnaround(), 57, "57 / 4 = 14.25");
    }

    #[test]
    fn srtf_average_wait_is_6_5_and_p1_is_split() {
        let s = simulate(&course(), Policy::Srtf);
        assert_eq!(s.gantt, [sl(0, 0, 1), sl(1, 1, 5), sl(3, 5, 10), sl(0, 10, 17), sl(2, 17, 26)]);
        assert_eq!(waits(&s), [9, 0, 15, 2], "P1 waits 17 - 8 = 9, not 10 (the trap in the notes)");
        assert_eq!(s.total_waiting(), 26, "26 / 4 = 6.5");
        assert_eq!(s.total_turnaround(), 52, "52 / 4 = 13.0");
    }

    #[test]
    fn srtf_is_never_worse_than_the_others_on_waiting() {
        let best = simulate(&course(), Policy::Srtf).total_waiting();
        for pol in [Policy::Fcfs, Policy::Sjf, Policy::RoundRobin { quantum: 4 }, Policy::Priority { preemptive: true }] {
            assert!(best <= simulate(&course(), pol).total_waiting(), "{pol:?}");
        }
    }

    #[test]
    fn round_robin_quantum_4_matches_the_notes() {
        let s = simulate(&course(), Policy::RoundRobin { quantum: 4 });
        assert_eq!(s.gantt, [sl(0, 0, 4), sl(1, 4, 8), sl(2, 8, 12), sl(3, 12, 16), sl(0, 16, 20), sl(2, 20, 24), sl(3, 24, 25), sl(2, 25, 26)]);
        assert_eq!(waits(&s), [12, 3, 15, 17]);
        assert_eq!(s.total_waiting(), 47, "47 / 4 = 11.75");
        assert_eq!(s.total_turnaround(), 73, "73 / 4 = 18.25");
        assert_eq!((0..4).map(|p| s.response(p)).collect::<Vec<_>>(), [0, 3, 6, 9]);
        assert_eq!(s.total_response(), 18, "first dispatches 0, 4, 8, 12 minus arrivals 0, 1, 2, 3");
    }

    /// The notes' preemptive-priority example (Chapter 5, Example 5) has an arithmetic slip: it shows P1
    /// running 0-1 and 5-13 (9 ticks for an 8-tick job) and P3 finishing at 27 (26 ticks of work, no
    /// idle time). The correct run, which obeys the notes' own cross-check, is below.
    #[test]
    fn preemptive_priority_correct_run() {
        let s = simulate(&course(), Policy::Priority { preemptive: true });
        assert_eq!(s.gantt, [sl(0, 0, 1), sl(1, 1, 5), sl(0, 5, 12), sl(3, 12, 17), sl(2, 17, 26)]);
        assert_eq!(waits(&s), [4, 0, 15, 9]);
        assert_eq!(s.total_waiting(), 28, "28 / 4 = 7.0 (the notes say 31 / 4 = 7.75)");
        assert_eq!(s.total_turnaround(), 54);
        assert_eq!(s.total_turnaround(), s.total_waiting() + 26, "turnaround = waiting + bursts");
    }

    #[test]
    fn non_preemptive_priority_lets_the_running_job_finish() {
        let s = simulate(&course(), Policy::Priority { preemptive: false });
        // P1 runs 0-8 although P2 is more important; then P2 (1), P4 (3), P3 (4).
        assert_eq!(s.gantt, [sl(0, 0, 8), sl(1, 8, 12), sl(3, 12, 17), sl(2, 17, 26)]);
    }

    #[test]
    fn idle_gaps_and_empty_input() {
        let late = [Proc { arrival: 5, burst: 2, priority: 0 }, Proc { arrival: 20, burst: 3, priority: 0 }];
        for pol in [Policy::Fcfs, Policy::Sjf, Policy::Srtf, Policy::Priority { preemptive: true }, Policy::RoundRobin { quantum: 2 }] {
            let s = simulate(&late, pol);
            assert_eq!(s.gantt, [sl(0, 5, 7), sl(1, 20, 23)], "{pol:?}");
            assert_eq!(s.total_waiting(), 0);
        }
        assert!(simulate(&[], Policy::Fcfs).is_empty());
        assert!(simulate(&[], Policy::RoundRobin { quantum: 0 }).gantt.is_empty());
    }

    #[test]
    fn zero_burst_finishes_on_arrival_and_zero_quantum_acts_as_one() {
        let ps = [Proc { arrival: 3, burst: 0, priority: 0 }, Proc { arrival: 0, burst: 2, priority: 0 }];
        let s = simulate(&ps, Policy::Fcfs);
        assert_eq!(s.completion(0), 3);
        assert_eq!(s.waiting(0), 0);
        let rr = simulate(&ps, Policy::RoundRobin { quantum: 0 });
        assert_eq!(rr.gantt, [sl(1, 0, 2)], "slices of 1 tick that follow each other join up");
    }

    /// Every policy gives every process exactly its burst, and the course's cross-check always holds.
    #[test]
    fn random_workloads_obey_the_books() {
        let mut seed = 7u32;
        let mut next = |m: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) % m
        };
        for _ in 0..200 {
            let n = 1 + next(6) as usize;
            let ps: Vec<Proc> = (0..n).map(|_| Proc { arrival: next(12), burst: next(8), priority: next(4) }).collect();
            for pol in [Policy::Fcfs, Policy::Sjf, Policy::Srtf, Policy::Priority { preemptive: false }, Policy::Priority { preemptive: true }, Policy::RoundRobin { quantum: 1 + next(5) }] {
                let s = simulate(&ps, pol);
                for (i, p) in ps.iter().enumerate() {
                    let ran: u32 = s.gantt.iter().filter(|x| x.proc == i).map(|x| x.end - x.start).sum();
                    assert_eq!(ran, p.burst, "{pol:?} {ps:?}");
                    assert!(s.completion(i) >= p.arrival + p.burst);
                }
                assert!(s.gantt.windows(2).all(|w| w[0].end <= w[1].start), "one CPU: slices never overlap");
                assert!(s.gantt.iter().all(|x| x.start >= ps[x.proc].arrival), "nobody runs before it arrives");
                let bursts: u64 = ps.iter().map(|p| u64::from(p.burst)).sum();
                assert_eq!(s.total_turnaround(), s.total_waiting() + bursts);
            }
        }
    }
}
