//! Banker's algorithm (deadlock avoidance) and deadlock detection. Pure and host-testable: no kernel
//! state, only numbers in and a verdict out. The test cases are the worked examples of the OS course
//! notes (Chapter 8, Examples 1 to 3), so the code is checked against numbers a person computed by hand.
//!
//! Vocabulary: there are `m` resource types and `n` processes. `Max` is what a process may ever ask
//! for, `Allocation` what it holds now, `Need = Max - Allocation`, `Available` what is free.

// Index loops read more like the textbook algorithms (`for each process i ...`) than iterator chains.
#![allow(clippy::needless_range_loop)]

use alloc::vec;
use alloc::vec::Vec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BankerError {
    /// The vectors and matrices do not all have the same number of resource types or processes.
    BadShape,
    /// A process holds more of some resource than its declared maximum.
    AllocationAboveMax,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestError {
    /// No such process, or the request has the wrong number of resource types.
    BadShape,
    /// The process asks for more than it declared (Request > Need).
    ExceedsNeed,
    /// Not enough free right now (Request > Available): the process must wait.
    NotAvailable,
    /// Granting it would leave no safe order: the process must wait. The state is unchanged.
    Unsafe,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    available: Vec<u32>,
    allocation: Vec<Vec<u32>>,
    max: Vec<Vec<u32>>,
}

fn fits(need: &[u32], work: &[u32]) -> bool {
    need.iter().zip(work).all(|(n, w)| n <= w)
}

impl State {
    /// Builds a state, checking that every row has `available.len()` columns and that no process holds
    /// more than it declared.
    pub fn new(available: Vec<u32>, allocation: Vec<Vec<u32>>, max: Vec<Vec<u32>>) -> Result<State, BankerError> {
        let m = available.len();
        if allocation.len() != max.len() || allocation.iter().chain(max.iter()).any(|r| r.len() != m) {
            return Err(BankerError::BadShape);
        }
        if allocation.iter().zip(&max).any(|(a, x)| a.iter().zip(x).any(|(a, x)| a > x)) {
            return Err(BankerError::AllocationAboveMax);
        }
        Ok(State { available, allocation, max })
    }

    pub fn processes(&self) -> usize {
        self.allocation.len()
    }

    pub fn available(&self) -> &[u32] {
        &self.available
    }

    /// `Need = Max - Allocation` for process `p`.
    pub fn need(&self, p: usize) -> Vec<u32> {
        self.max[p].iter().zip(&self.allocation[p]).map(|(x, a)| x - a).collect()
    }

    /// The safety algorithm. Returns an order in which every process can finish, or `None` when the
    /// state is unsafe. Each pass rescans every unfinished process from the top (a process that failed
    /// early in a pass can succeed in the next), exactly as in the course notes.
    pub fn safe_sequence(&self) -> Option<Vec<usize>> {
        let n = self.processes();
        let mut work = self.available.clone();
        let mut done = vec![false; n];
        let mut order = Vec::with_capacity(n);
        loop {
            let mut progress = false;
            for p in 0..n {
                if !done[p] && fits(&self.need(p), &work) {
                    for (w, a) in work.iter_mut().zip(&self.allocation[p]) {
                        *w += a;
                    }
                    done[p] = true;
                    order.push(p);
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
        (order.len() == n).then_some(order)
    }

    pub fn is_safe(&self) -> bool {
        self.safe_sequence().is_some()
    }

    /// Process `p` asks for `req` more. Checks Request <= Need, then Request <= Available, then pretends
    /// to grant it and keeps the new state only when that state is safe. On success returns the safe
    /// order found. On any error the state is left exactly as it was.
    pub fn request(&mut self, p: usize, req: &[u32]) -> Result<Vec<usize>, RequestError> {
        if p >= self.processes() || req.len() != self.available.len() {
            return Err(RequestError::BadShape);
        }
        if !fits(req, &self.need(p)) {
            return Err(RequestError::ExceedsNeed);
        }
        if !fits(req, &self.available) {
            return Err(RequestError::NotAvailable);
        }
        let mut next = self.clone();
        for (j, r) in req.iter().enumerate() {
            next.available[j] -= r;
            next.allocation[p][j] += r;
        }
        let order = next.safe_sequence().ok_or(RequestError::Unsafe)?;
        *self = next;
        Ok(order)
    }
}

/// Deadlock detection for several instances per resource type. Uses what each process is waiting for
/// right now (`request`), not what it might want later. Returns the indexes of the deadlocked
/// processes, in order (empty when there is no deadlock). A process waiting on a deadlocked process is
/// itself deadlocked, so it is included. All rows must have `available.len()` columns, and
/// `allocation` and `request` must have the same number of rows (otherwise nothing is reported).
pub fn deadlocked(available: &[u32], allocation: &[Vec<u32>], request: &[Vec<u32>]) -> Vec<usize> {
    let n = allocation.len();
    let m = available.len();
    if request.len() != n || allocation.iter().chain(request.iter()).any(|r| r.len() != m) {
        return Vec::new();
    }
    let mut work = available.to_vec();
    // A process that holds nothing cannot be part of a deadlock.
    let mut finish: Vec<bool> = allocation.iter().map(|a| a.iter().all(|&x| x == 0)).collect();
    loop {
        let mut progress = false;
        for p in 0..n {
            if !finish[p] && fits(&request[p], &work) {
                for (w, a) in work.iter_mut().zip(&allocation[p]) {
                    *w += a;
                }
                finish[p] = true;
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    (0..n).filter(|&p| !finish[p]).collect()
}

/// Wait-for graph detection for single-instance resources: `edges[p]` lists the processes that `p` is
/// waiting for. Returns the processes that are on a cycle or wait (directly or not) on one, in order.
pub fn deadlocked_in_wait_for_graph(edges: &[Vec<usize>]) -> Vec<usize> {
    let n = edges.len();
    // A process is free when everything it waits for is free (a process waiting for nothing is free).
    // Repeating this until nothing changes leaves exactly the processes that are stuck forever.
    let mut free = vec![false; n];
    loop {
        let mut progress = false;
        for p in 0..n {
            if !free[p] && edges[p].iter().all(|&q| q < n && free[q]) {
                free[p] = true;
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    (0..n).filter(|&p| !free[p]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Course Chapter 8, Example 1: five processes, three resource types, totals (10, 5, 7).
    fn example1() -> State {
        State::new(
            vec![3, 3, 2],
            vec![vec![0, 1, 0], vec![2, 0, 0], vec![3, 0, 2], vec![2, 1, 1], vec![0, 0, 2]],
            vec![vec![7, 5, 3], vec![3, 2, 2], vec![9, 0, 2], vec![2, 2, 2], vec![4, 3, 3]],
        )
        .unwrap()
    }

    #[test]
    fn example1_need_matrix_and_safe_sequence() {
        let s = example1();
        let need: Vec<_> = (0..5).map(|p| s.need(p)).collect();
        assert_eq!(need, [vec![7, 4, 3], vec![1, 2, 2], vec![6, 0, 0], vec![0, 1, 1], vec![4, 3, 1]]);
        // The notes find <P1, P3, P4, P0, P2> by rescanning from P0 after every pass.
        assert_eq!(s.safe_sequence(), Some(vec![1, 3, 4, 0, 2]));
    }

    #[test]
    fn example2_request_is_granted_and_state_changes_in_three_places() {
        let mut s = example1();
        assert_eq!(s.request(1, &[1, 0, 2]), Ok(vec![1, 3, 4, 0, 2]));
        assert_eq!(s.available(), [2, 3, 0]);
        assert_eq!(s.allocation[1], [3, 0, 2]);
        assert_eq!(s.need(1), [0, 2, 0]);
    }

    #[test]
    fn example2_the_two_contrasts_from_the_notes() {
        let mut s = example1();
        s.request(1, &[1, 0, 2]).unwrap();
        let before = s.clone();
        // Check 2 fails: asks for 3 of A but only 2 are free. P4 waits.
        assert_eq!(s.request(4, &[3, 3, 0]), Err(RequestError::NotAvailable));
        // Both checks pass but no process could finish afterwards: unsafe, rolled back, P0 waits.
        assert_eq!(s.request(0, &[0, 2, 0]), Err(RequestError::Unsafe));
        assert_eq!(s, before, "a refused request changes nothing");
    }

    #[test]
    fn request_above_need_and_bad_shapes_are_refused() {
        let mut s = example1();
        assert_eq!(s.request(1, &[2, 0, 0]), Err(RequestError::ExceedsNeed), "P1 needs at most (1,2,2)");
        assert_eq!(s.request(9, &[0, 0, 0]), Err(RequestError::BadShape));
        assert_eq!(s.request(1, &[0, 0]), Err(RequestError::BadShape));
        assert_eq!(s, example1());
    }

    #[test]
    fn final_work_equals_the_totals() {
        // The notes' ground-truth check: after everything finishes, free = totals = (10, 5, 7).
        let s = example1();
        let mut total = s.available().to_vec();
        for a in &s.allocation {
            for (t, x) in total.iter_mut().zip(a) {
                *t += x;
            }
        }
        assert_eq!(total, [10, 5, 7]);
    }

    #[test]
    fn an_unsafe_state_has_no_sequence() {
        // One resource, 1 free, two processes each holding 1 and needing 2 more: nobody can finish.
        let s = State::new(vec![1], vec![vec![1], vec![1]], vec![vec![3], vec![3]]).unwrap();
        assert_eq!(s.safe_sequence(), None);
        assert!(!s.is_safe());
    }

    #[test]
    fn new_rejects_bad_input() {
        assert_eq!(State::new(vec![1, 1], vec![vec![0]], vec![vec![1, 1]]).unwrap_err(), BankerError::BadShape);
        assert_eq!(State::new(vec![1], vec![vec![0]], vec![]).unwrap_err(), BankerError::BadShape);
        assert_eq!(State::new(vec![1], vec![vec![2]], vec![vec![1]]).unwrap_err(), BankerError::AllocationAboveMax);
        assert!(State::new(vec![], vec![], vec![]).unwrap().is_safe(), "nothing to run is trivially safe");
    }

    /// Course Chapter 8, Example 3: detection with the Request matrix.
    fn detection_allocation() -> Vec<Vec<u32>> {
        vec![vec![0, 1, 0], vec![2, 0, 0], vec![3, 0, 3], vec![2, 1, 1], vec![0, 0, 2]]
    }

    #[test]
    fn example3_no_deadlock() {
        let req = vec![vec![0, 0, 0], vec![2, 0, 2], vec![0, 0, 0], vec![1, 0, 0], vec![0, 0, 2]];
        assert!(deadlocked(&[0, 0, 0], &detection_allocation(), &req).is_empty());
    }

    #[test]
    fn example3_one_more_instance_of_c_freezes_four_processes() {
        let req = vec![vec![0, 0, 0], vec![2, 0, 2], vec![0, 0, 1], vec![1, 0, 0], vec![0, 0, 2]];
        // P0 escaped because it asks for nothing; P1 to P4 are deadlocked.
        assert_eq!(deadlocked(&[0, 0, 0], &detection_allocation(), &req), [1, 2, 3, 4]);
    }

    #[test]
    fn detection_ignores_a_process_that_holds_nothing() {
        // P0 holds nothing and wants something that is not free: it is not deadlocked.
        assert!(deadlocked(&[0], &[vec![0]], &[vec![1]]).is_empty());
        assert!(deadlocked(&[0], &[vec![0]], &[]).is_empty(), "bad shape reports nothing");
    }

    #[test]
    fn wait_for_graph_example_from_the_notes() {
        // Edges: P1 -> P2, P2 -> P3, P3 -> P1, P4 -> P3. Indexes 0..3 stand for P1..P4.
        let g = vec![vec![1], vec![2], vec![0], vec![2]];
        // P1, P2, P3 are on the cycle; P4 waits on a deadlocked process, so it is stuck too.
        assert_eq!(deadlocked_in_wait_for_graph(&g), [0, 1, 2, 3]);
        // Break the cycle (P3 waits for nobody): nobody is stuck.
        let g = vec![vec![1], vec![2], vec![], vec![2]];
        assert!(deadlocked_in_wait_for_graph(&g).is_empty());
        // A process waiting on a missing process is stuck.
        assert_eq!(deadlocked_in_wait_for_graph(&[vec![7]]), [0]);
    }

    /// Random states never panic, and a granted request always leaves a safe state.
    #[test]
    fn random_requests_never_break_safety() {
        let mut seed = 0xBEEF_u32;
        let mut next = |m: u32| {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (seed >> 16) % m
        };
        for _ in 0..300 {
            let (n, m) = (1 + next(5) as usize, 1 + next(3) as usize);
            let max: Vec<Vec<u32>> = (0..n).map(|_| (0..m).map(|_| next(6)).collect()).collect();
            let alloc: Vec<Vec<u32>> = max.iter().map(|r| r.iter().map(|&x| next(x + 1)).collect()).collect();
            let avail: Vec<u32> = (0..m).map(|_| next(5)).collect();
            let mut s = State::new(avail, alloc, max).unwrap();
            let was_safe = s.is_safe();
            for _ in 0..10 {
                let p = next(n as u32) as usize;
                let req: Vec<u32> = (0..m).map(|_| next(3)).collect();
                if s.request(p, &req).is_ok() && was_safe {
                    assert!(s.is_safe(), "granting from a safe state must stay safe");
                }
            }
        }
    }
}
