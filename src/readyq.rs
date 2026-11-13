//! Zenkai OS - Priority Task Ready Queue
//!
//! Author: Swapnil Ghosh 25BTRCL267 AIML-E
//! CA-6 Layer 5: Ready Queue Architecture & Task Enqueue/Dequeue Dispatch.

use crate::task::{Task, TaskState};

pub const MAX_TASKS: usize = 16;

pub struct ReadyQueue {
    tasks: [Option<Task>; MAX_TASKS],
    count: usize,
}

impl ReadyQueue {
    pub const fn new() -> Self {
        Self {
            tasks: [None; MAX_TASKS],
            count: 0,
        }
    }

    pub fn push(&mut self, mut task: Task) -> bool {
        if self.count >= MAX_TASKS {
            return false;
        }

        task.state = TaskState::Ready;

        // Insert maintaining priority order (higher priority first)
        let mut insert_idx = self.count;
        for i in 0..self.count {
            if let Some(existing) = &self.tasks[i] {
                if task.priority > existing.priority {
                    insert_idx = i;
                    break;
                }
            }
        }

        // Shift elements right
        for i in (insert_idx..self.count).rev() {
            self.tasks[i + 1] = self.tasks[i];
        }

        self.tasks[insert_idx] = Some(task);
        self.count += 1;
        true
    }

    pub fn pop(&mut self) -> Option<Task> {
        if self.count == 0 {
            return None;
        }

        let task = self.tasks[0].take();
        // Shift remaining left
        for i in 0..self.count - 1 {
            self.tasks[i] = self.tasks[i + 1].take();
        }
        self.count -= 1;
        task
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = &Task> {
        self.tasks[..self.count].iter().filter_map(|t| t.as_ref())
    }
}

