//! Task Representation and Process Control Block (PCB) (CA-6 Layer 3)
//!
//! Defines execution states (READY, RUNNING, BLOCKED, TERMINATED) and
//! maintains scheduling metadata and execution metrics per task.

extern crate alloc;
use alloc::string::String;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Terminated,
}

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Ready => "READY",
            TaskState::Running => "RUNNING",
            TaskState::Blocked => "BLOCKED",
            TaskState::Terminated => "TERMINATED",
        }
    }
}

pub type TaskId = u32;

/// Process Control Block (PCB)
#[derive(Debug, Clone)]
pub struct TaskControlBlock {
    pub id: TaskId,
    pub name: String,
    pub state: TaskState,
    pub priority: u8,
    pub quantum_remaining: usize,
    pub work_units_remaining: usize,
    pub total_ticks_run: usize,
    pub context_switches: usize,
}

impl TaskControlBlock {
    /// Create a new runnable task
    pub fn new(id: TaskId, name: String, priority: u8, work_units: usize) -> Self {
        Self {
            id,
            name,
            state: TaskState::Ready,
            priority,
            quantum_remaining: 2, // 2-tick quantum
            work_units_remaining: work_units,
            total_ticks_run: 0,
            context_switches: 0,
        }
    }

    /// Execute one time slice of work
    /// Returns true if task completes and transitions to TERMINATED
    pub fn execute_slice(&mut self, quantum: usize) -> bool {
        self.state = TaskState::Running;
        let units_to_run = self.work_units_remaining.min(quantum);

        self.work_units_remaining -= units_to_run;
        self.total_ticks_run += units_to_run;
        self.context_switches += 1;

        if self.work_units_remaining == 0 {
            self.state = TaskState::Terminated;
            true
        } else {
            self.state = TaskState::Ready;
            false
        }
    }
}

