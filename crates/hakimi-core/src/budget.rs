use std::sync::atomic::{AtomicUsize, Ordering};

/// A thread-safe iteration budget that tracks how many iterations have been used.
///
/// Used to cap the number of tool-calling loops in a conversation.
pub struct IterationBudget {
    max: usize,
    used: AtomicUsize,
}

impl IterationBudget {
    /// Create a new budget with the given maximum number of iterations.
    pub fn new(max: usize) -> Self {
        Self {
            max,
            used: AtomicUsize::new(0),
        }
    }

    /// Returns the number of iterations remaining.
    pub fn remaining(&self) -> usize {
        self.max.saturating_sub(self.used.load(Ordering::Relaxed))
    }

    /// Consume one iteration. Returns `true` if the iteration was within budget.
    pub fn use_one(&self) -> bool {
        let prev = self.used.fetch_add(1, Ordering::Relaxed);
        prev < self.max
    }

    /// Returns `true` if the budget has been exhausted.
    pub fn is_exhausted(&self) -> bool {
        self.used.load(Ordering::Relaxed) >= self.max
    }
}
