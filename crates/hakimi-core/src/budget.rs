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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_budget_new() {
        let budget = IterationBudget::new(5);
        assert_eq!(budget.remaining(), 5);
        assert!(!budget.is_exhausted());
    }

    #[test]
    fn test_budget_use_one() {
        let budget = IterationBudget::new(3);
        assert_eq!(budget.remaining(), 3);
        budget.use_one();
        assert_eq!(budget.remaining(), 2);
        budget.use_one();
        assert_eq!(budget.remaining(), 1);
    }

    #[test]
    fn test_budget_exhausted() {
        let budget = IterationBudget::new(2);
        assert!(!budget.is_exhausted());
        budget.use_one();
        assert!(!budget.is_exhausted());
        budget.use_one();
        assert!(budget.is_exhausted());
    }

    #[test]
    fn test_budget_use_one_returns_false_when_exhausted() {
        let budget = IterationBudget::new(1);
        assert!(budget.use_one());
        assert!(!budget.use_one());
        assert!(!budget.use_one());
    }

    #[test]
    fn test_budget_zero_max() {
        let budget = IterationBudget::new(0);
        assert_eq!(budget.remaining(), 0);
        assert!(budget.is_exhausted());
        assert!(!budget.use_one());
    }

    #[test]
    fn test_budget_remaining_saturates_at_zero() {
        let budget = IterationBudget::new(2);
        budget.use_one();
        budget.use_one();
        budget.use_one(); // over budget
        budget.use_one(); // over budget
        assert_eq!(budget.remaining(), 0);
    }
}
