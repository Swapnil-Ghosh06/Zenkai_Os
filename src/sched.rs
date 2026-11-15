//! Zenkai OS - Round-Robin Preemptive Task Scheduler
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! CA-6 Layer 6: Time-Slice Quantum, Preemption & Context Switch Emulation.

extern crate alloc;
use alloc::string::String;

use crate::readyq::ReadyQueue;
use crate::task::{Task, TaskState};

pub const DEFAULT_QUANTUM: usize = 4;

pub struct Scheduler {
    pub ready_queue: ReadyQueue,
    pub current_task: Option<Task>,
    pub quantum: usize,
    pub total_context_switches: usize,
    pub total_ticks: usize,
    pub next_pid: usize,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            ready_queue: ReadyQueue::new(),
            current_task: None,
            quantum: DEFAULT_QUANTUM,
            total_context_switches: 0,
            total_ticks: 0,
            next_pid: 4,
        }
    }

    pub fn add_task(&mut self, task: Task) -> bool {
        self.ready_queue.push(task)
    }

    pub fn spawn(&mut self, name: &str, priority: u8, burst: usize) -> usize {
        let pid = self.next_pid;
        self.next_pid += 1;
        let task = Task::from_str(pid, name, priority, burst);
        self.ready_queue.push(task);
        pid
    }

    pub fn kill(&mut self, pid: usize) -> bool {
        if let Some(cur) = &self.current_task {
            if cur.pid == pid {
                self.current_task = None;
                self.total_context_switches += 1;
                return true;
            }
        }
        self.ready_queue.remove_by_pid(pid).is_some()
    }

    pub fn set_nice(&mut self, pid: usize, new_priority: u8) -> bool {
        if let Some(cur) = &mut self.current_task {
            if cur.pid == pid {
                cur.priority = new_priority;
                return true;
            }
        }
        if let Some(mut task) = self.ready_queue.remove_by_pid(pid) {
            task.priority = new_priority;
            self.ready_queue.push(task);
            return true;
        }
        false
    }

    pub fn set_quantum(&mut self, q: usize) {
        if q > 0 {
            self.quantum = q;
        }
    }

    pub fn schedule_step(&mut self) -> Option<(String, usize, TaskState)> {
        self.total_ticks += 1;

        // If no task running, pick one from ready queue
        if self.current_task.is_none() {
            if let Some(mut next) = self.ready_queue.pop() {
                next.state = TaskState::Running;
                next.context_switches += 1;
                self.current_task = Some(next);
                self.total_context_switches += 1;
            } else {
                return None;
            }
        }

        let task = self.current_task.as_mut().unwrap();
        task.burst_remaining = task.burst_remaining.saturating_sub(1);
        task.total_runtime += 1;

        let name = String::from(task.name());
        let pid = task.pid;

        if task.is_completed() {
            task.state = TaskState::Terminated;
            self.current_task = None;
            Some((name, pid, TaskState::Terminated))
        } else if task.total_runtime % self.quantum == 0 {
            // Quantum expired: preemption
            let mut preempted = self.current_task.take().unwrap();
            preempted.state = TaskState::Ready;
            self.ready_queue.push(preempted);
            self.total_context_switches += 1;
            Some((name, pid, TaskState::Ready))
        } else {
            Some((name, pid, TaskState::Running))
        }
    }
}
