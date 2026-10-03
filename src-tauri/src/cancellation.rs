use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const CANCELLED: &str = "Transcription cancelled";

/// Drop an in-flight HTTP operation promptly when the user cancels dictation.
pub async fn run<T>(
    cancelled: &AtomicBool,
    operation: impl Future<Output = Result<T, String>>,
) -> Result<T, String> {
    tokio::select! {
        biased;
        _ = async {
            while !cancelled.load(Ordering::SeqCst) {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        } => Err(CANCELLED.to_string()),
        result = operation => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_wins_over_an_already_ready_result() {
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            run(&cancelled, async { Ok(42) }).await,
            Err(CANCELLED.into())
        );
    }

    #[tokio::test]
    async fn cancellation_interrupts_a_pending_operation() {
        let cancelled = AtomicBool::new(false);
        let operation = async {
            cancelled.store(true, Ordering::SeqCst);
            std::future::pending::<Result<(), String>>().await
        };
        let result = tokio::time::timeout(Duration::from_secs(1), run(&cancelled, operation)).await;
        assert_eq!(result.unwrap(), Err(CANCELLED.into()));
    }

    #[tokio::test]
    async fn successful_operations_return_their_result() {
        assert_eq!(run(&AtomicBool::new(false), async { Ok(42) }).await, Ok(42));
    }
}
