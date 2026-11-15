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

impl TaskState {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Ready => "Ready",
            TaskState::Running => "Running",
            TaskState::Blocked => "Blocked",
            TaskState::Terminated => "Terminated",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Task {
    pub pid: usize,
    pub name: [u8; 16],
    pub name_len: usize,
    pub priority: u8, // 1 (lowest) to 10 (highest)
    pub state: TaskState,
    pub burst_remaining: usize,
    pub total_runtime: usize,
    pub context_switches: usize,
}

impl Task {
    pub const fn new(pid: usize, name_str: &'static str, priority: u8, burst: usize) -> Self {
        let bytes = name_str.as_bytes();
        let mut name = [0u8; 16];
        let mut i = 0;
        while i < bytes.len() && i < 16 {
            name[i] = bytes[i];
            i += 1;
        }
        Self {
            pid,
            name,
            name_len: i,
            priority,
            state: TaskState::Ready,
            burst_remaining: burst,
            total_runtime: 0,
            context_switches: 0,
        }
    }

    pub fn from_str(pid: usize, s: &str, priority: u8, burst: usize) -> Self {
        let bytes = s.as_bytes();
        let mut name = [0u8; 16];
        let len = bytes.len().min(16);
        name[..len].copy_from_slice(&bytes[..len]);
        Self {
            pid,
            name,
            name_len: len,
            priority,
            state: TaskState::Ready,
            burst_remaining: burst,
            total_runtime: 0,
            context_switches: 0,
        }
    }

    pub fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_len]).unwrap_or("task")
    }

    pub fn is_completed(&self) -> bool {
        self.burst_remaining == 0
    }
}
