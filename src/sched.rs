//! Round Robin CPU Scheduler (CA-6 Layer 5 & 6)
//!
//! Implements a time-sliced Round Robin scheduler with quantum enforcement,
//! context switch tracing, and lifecycle management for runnable tasks.

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::kinfo;
use crate::kok;
use crate::ktrace;
use crate::readyq::ReadyQueue;
use crate::task::{TaskControlBlock, TaskId};

pub struct Scheduler {
    ready_queue: ReadyQueue,
    quantum: usize,
    total_context_switches: usize,
    total_ticks_elapsed: usize,
    terminated_tasks: Vec<TaskControlBlock>,
}

impl Scheduler {
    pub fn new(quantum: usize) -> Self {
        Self {
            ready_queue: ReadyQueue::new(),
            quantum,
            total_context_switches: 0,
            total_ticks_elapsed: 0,
            terminated_tasks: Vec::new(),
        }
    }

    /// Add a new task to the scheduler's ready queue
    pub fn add_task(&mut self, id: TaskId, name: String, priority: u8, work_units: usize) {
        let task = TaskControlBlock::new(id, name, priority, work_units);
        self.ready_queue.enqueue(task);
    }

    /// Execute the Round Robin scheduling loop until all tasks complete
    pub fn run_all(&mut self) {
        kinfo!("SCHED", "Starting Round Robin Scheduler (Quantum = {} ticks)...", self.quantum);

        while let Some(mut current_task) = self.ready_queue.dequeue() {
            self.total_context_switches += 1;
            ktrace!("SCHED", "DISPATCH -> Task #{}: '{}' (state: {}) | work remaining: {}",
                    current_task.id, current_task.name, current_task.state.as_str(),
                    current_task.work_units_remaining);

            // Execute time slice
            let finished = current_task.execute_slice(self.quantum);
            let ticks_run = self.quantum.min(current_task.total_ticks_run);
            self.total_ticks_elapsed += ticks_run;

            if finished {
                kok!("SCHED", "COMPLETED -> Task #{}: '{}' in {} ticks ({} context switches)",
                     current_task.id, current_task.name,
                     current_task.total_ticks_run, current_task.context_switches);
                self.terminated_tasks.push(current_task);
            } else {
                ktrace!("SCHED", "YIELD/PREEMPT -> Task #{}: '{}' time slice expired, re-enqueueing",
                        current_task.id, current_task.name);
                self.ready_queue.enqueue(current_task);
            }
        }

        kok!("SCHED", "All tasks completed successfully. Total context switches: {}, Total ticks: {}",
             self.total_context_switches, self.total_ticks_elapsed);
    }

    /// Print summary table of scheduling results
    pub fn print_summary(&self) {
        kinfo!("SCHED", "=================== SCHEDULER EXECUTION REPORT ===================");
        kinfo!("SCHED", "{:<6} | {:<20} | {:<10} | {:<16}", "TaskID", "Name", "Ticks Run", "Context Switches");
        kinfo!("SCHED", "------------------------------------------------------------------");
        for t in &self.terminated_tasks {
            kinfo!("SCHED", "#{:<5} | {:<20} | {:<10} | {:<16}",
                   t.id, t.name, t.total_ticks_run, t.context_switches);
        }
        kinfo!("SCHED", "==================================================================");
    }
}

/// Run an empirical demonstration of Round Robin multi-task scheduling
pub fn run_scheduler_demo() {
    kinfo!("SCHED", "=== CA-6 Layer 5: Round Robin Scheduling Demonstration ===");
    let mut sched = Scheduler::new(2); // Quantum = 2 ticks

    sched.add_task(1, String::from("TensorStreamLoader"), 1, 6);
    sched.add_task(2, String::from("ConvKernelWorker"),   1, 4);
    sched.add_task(3, String::from("MemoryAuditDaemon"),  2, 2);

    sched.run_all();
    sched.print_summary();
}
