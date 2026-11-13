//! Ready Queue Kernel Data Structure (CA-6 Layer 4)
//!
//! Provides a runnable task queue managing task insertion, FIFO selection,
//! state inspection, and removal for CPU scheduling.

extern crate alloc;
use alloc::collections::VecDeque;
use crate::task::{TaskControlBlock, TaskId, TaskState};

pub struct ReadyQueue {
    tasks: VecDeque<TaskControlBlock>,
}

impl ReadyQueue {
    pub fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
        }
    }

    /// Insert a task into the back of the ready queue
    pub fn enqueue(&mut self, mut task: TaskControlBlock) {
        task.state = TaskState::Ready;
        self.tasks.push_back(task);
    }

    /// Dequeue the next runnable task from the front
    pub fn dequeue(&mut self) -> Option<TaskControlBlock> {
        self.tasks.pop_front()
    }

    /// Peek at the head task without removing it
    pub fn peek(&self) -> Option<&TaskControlBlock> {
        self.tasks.front()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// Get total number of runnable tasks
    pub fn len(&self) -> usize {
        self.tasks.len()
    }

    /// Search for a task by ID
    pub fn find_by_id(&self, id: TaskId) -> Option<&TaskControlBlock> {
        self.tasks.iter().find(|t| t.id == id)
    }

    /// Read-only slice/iterator for diagnostics
    pub fn iter(&self) -> impl Iterator<Item = &TaskControlBlock> {
        self.tasks.iter()
    }
}

