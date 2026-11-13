//! Zenkai OS - Round-Robin Preemptive Task Scheduler
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! CA-6 Layer 6: Time-Slice Quantum, Preemption & Context Switch Emulation.

use crate::readyq::ReadyQueue;
use crate::task::{Task, TaskState};

pub const DEFAULT_QUANTUM: usize = 4;

pub struct Scheduler {
    pub ready_queue: ReadyQueue,
    pub current_task: Option<Task>,
    pub quantum: usize,
    pub total_context_switches: usize,
    pub total_ticks: usize,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            ready_queue: ReadyQueue::new(),
            current_task: None,
            quantum: DEFAULT_QUANTUM,
            total_context_switches: 0,
            total_ticks: 0,
        }
    }

    pub fn add_task(&mut self, task: Task) -> bool {
        self.ready_queue.push(task)
    }

    pub fn schedule_step(&mut self) -> Option<(&'static str, usize, TaskState)> {
        self.total_ticks += 1;

        // If no task running, pick one
        if self.current_task.is_none() {
            if let Some(mut next) = self.ready_queue.pop() {
                next.state = TaskState::Running;
                self.current_task = Some(next);
                self.total_context_switches += 1;
            } else {
                return None;
            }
        }

        let task = self.current_task.as_mut().unwrap();
        task.burst_remaining = task.burst_remaining.saturating_sub(1);
        task.total_runtime += 1;

        let name = task.name;
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

