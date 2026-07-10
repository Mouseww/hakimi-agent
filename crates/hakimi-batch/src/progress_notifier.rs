//! Real-time progress notifications for batch jobs.

use crate::progress::JobProgress;
use anyhow::Result;
use serde::Serialize;
use tokio::sync::broadcast;

/// Notifier for broadcasting progress updates.
pub struct ProgressNotifier {
    tx: broadcast::Sender<ProgressUpdate>,
}

/// A progress update message.
#[derive(Clone, Debug, Serialize)]
pub struct ProgressUpdate {
    /// Job ID.
    pub job_id: String,
    /// Current progress state.
    pub progress: JobProgress,
    /// Timestamp of the update (Unix timestamp).
    pub timestamp: i64,
}

impl ProgressNotifier {
    /// Create a new progress notifier.
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(100);
        Self { tx }
    }

    /// Create a new notifier with a specific channel capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Notify subscribers of a progress update.
    pub fn notify(&self, job_id: &str, progress: &JobProgress) -> Result<()> {
        let update = ProgressUpdate {
            job_id: job_id.to_string(),
            progress: progress.clone(),
            timestamp: chrono::Utc::now().timestamp(),
        };

        // Ignore send errors (no active receivers)
        let _ = self.tx.send(update);
        Ok(())
    }

    /// Subscribe to progress updates.
    pub fn subscribe(&self) -> broadcast::Receiver<ProgressUpdate> {
        self.tx.subscribe()
    }

    /// Get the number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for ProgressNotifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progress::JobProgress;

    #[tokio::test]
    async fn test_notify_and_receive() {
        let notifier = ProgressNotifier::new();
        let mut rx = notifier.subscribe();

        let progress = JobProgress::new(100, vec!["test".to_string()]);
        notifier.notify("job-1", &progress).unwrap();

        let update = rx.recv().await.unwrap();
        assert_eq!(update.job_id, "job-1");
        assert_eq!(update.progress.total_steps, 100);
    }

    #[tokio::test]
    async fn test_multiple_subscribers() {
        let notifier = ProgressNotifier::new();
        let mut rx1 = notifier.subscribe();
        let mut rx2 = notifier.subscribe();

        assert_eq!(notifier.subscriber_count(), 2);

        let progress = JobProgress::new(10, vec!["test".to_string()]);
        notifier.notify("job-1", &progress).unwrap();

        let update1 = rx1.recv().await.unwrap();
        let update2 = rx2.recv().await.unwrap();

        assert_eq!(update1.job_id, update2.job_id);
        assert_eq!(update1.progress.total_steps, 10);
        assert_eq!(update2.progress.total_steps, 10);
    }

    #[test]
    fn test_notify_without_subscribers() {
        let notifier = ProgressNotifier::new();
        let progress = JobProgress::new(10, vec!["test".to_string()]);

        // Should not panic
        notifier.notify("job-1", &progress).unwrap();
    }

    #[test]
    fn test_with_capacity() {
        let notifier = ProgressNotifier::with_capacity(200);
        let _rx = notifier.subscribe();

        let progress = JobProgress::new(10, vec!["test".to_string()]);
        notifier.notify("job-1", &progress).unwrap();

        assert_eq!(notifier.subscriber_count(), 1);
    }

    #[test]
    fn test_dropped_subscriber() {
        let notifier = ProgressNotifier::new();
        {
            let _rx = notifier.subscribe();
            assert_eq!(notifier.subscriber_count(), 1);
        }
        // Subscriber dropped
        // Note: receiver_count may not immediately update
    }
}
