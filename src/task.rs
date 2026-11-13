//! Zenkai OS - Task Management & Process Control Block (PCB)
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! CA-6 Layers 3 & 4: Task State Machine, Priority Attributes & Execution Accounting.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Ready,
    Running,
    Blocked,
    Terminated,
}

#[derive(Debug, Clone, Copy)]
pub struct Task {
    pub pid: usize,
    pub name: &'static str,
    pub priority: u8, // 1 (lowest) to 10 (highest)
    pub state: TaskState,
    pub burst_remaining: usize,
    pub total_runtime: usize,
}

impl Task {
    pub const fn new(pid: usize, name: &'static str, priority: u8, burst: usize) -> Self {
        Self {
            pid,
            name,
            priority,
            state: TaskState::Ready,
            burst_remaining: burst,
            total_runtime: 0,
        }
    }

    pub fn is_completed(&self) -> bool {
        self.burst_remaining == 0
    }
}
