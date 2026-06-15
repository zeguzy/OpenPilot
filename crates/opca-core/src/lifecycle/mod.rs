pub mod heartbeat;
pub mod status;

pub use heartbeat::{Heartbeat, LifecycleTracker, SubTaskHeartbeat, TodoSummary};
pub use status::{ALL_STATUSES, TaskStatus, TransitionError, is_valid_transition, transition};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("task {task_id} panicked: {message}")]
pub struct TaskPanic {
    pub task_id: String,
    pub message: String,
}

pub async fn spawn_task<F, T>(task_id: impl Into<String>, fut: F) -> Result<T, TaskPanic>
where
    F: std::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let task_id = task_id.into();
    let handle = tokio::spawn(fut);
    match handle.await {
        Ok(result) => Ok(result),
        Err(join_error) => {
            let message = if join_error.is_panic() {
                let payload = join_error.into_panic();
                extract_panic_message(&*payload)
            } else {
                "task was cancelled".to_string()
            };
            Err(TaskPanic { task_id, message })
        }
    }
}

fn extract_panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&'static str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_string())
}
