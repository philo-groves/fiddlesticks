//! Standard retry/backoff policy and operational hook contracts.

use std::future::Future;
use std::time::Duration;

use crate::{ProviderError, ProviderId};

#[derive(Debug, Clone, PartialEq)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(5),
            backoff_multiplier: 2.0,
        }
    }
}

impl RetryPolicy {
    pub fn new(max_attempts: u32) -> Self {
        Self {
            max_attempts: max_attempts.max(1),
            ..Self::default()
        }
    }

    pub fn should_retry(&self, attempt: u32, error: &ProviderError) -> bool {
        error.retryable && attempt < self.max_attempts
    }

    pub fn backoff_for_attempt(&self, attempt: u32) -> Duration {
        let exponent = (attempt.saturating_sub(1)) as i32;
        let unbounded = self.initial_backoff.as_secs_f64() * self.backoff_multiplier.powi(exponent);
        Duration::from_secs_f64(unbounded.min(self.max_backoff.as_secs_f64()))
    }
}

pub trait ProviderOperationHooks: Send + Sync {
    fn on_attempt_start(&self, _provider: ProviderId, _operation: &str, _attempt: u32) {}

    fn on_retry_scheduled(
        &self,
        _provider: ProviderId,
        _operation: &str,
        _attempt: u32,
        _delay: Duration,
        _error: &ProviderError,
    ) {
    }

    fn on_success(&self, _provider: ProviderId, _operation: &str, _attempts: u32) {}

    fn on_failure(
        &self,
        _provider: ProviderId,
        _operation: &str,
        _attempts: u32,
        _error: &ProviderError,
    ) {
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopOperationHooks;

impl ProviderOperationHooks for NoopOperationHooks {}

pub async fn execute_with_retry<T, Op, OpFuture, Sleep, SleepFuture>(
    provider: ProviderId,
    operation: &str,
    policy: &RetryPolicy,
    hooks: &dyn ProviderOperationHooks,
    mut execute: Op,
    mut sleep: Sleep,
) -> Result<T, ProviderError>
where
    Op: FnMut(u32) -> OpFuture,
    OpFuture: Future<Output = Result<T, ProviderError>>,
    Sleep: FnMut(Duration) -> SleepFuture,
    SleepFuture: Future<Output = ()>,
{
    let mut attempt = 1;

    loop {
        hooks.on_attempt_start(provider, operation, attempt);

        match execute(attempt).await {
            Ok(value) => {
                hooks.on_success(provider, operation, attempt);
                return Ok(value);
            }
            Err(error) => {
                if policy.should_retry(attempt, &error) {
                    let delay = policy.backoff_for_attempt(attempt);
                    hooks.on_retry_scheduled(provider, operation, attempt, delay, &error);
                    sleep(delay).await;
                    attempt += 1;
                    continue;
                }

                hooks.on_failure(provider, operation, attempt, &error);
                return Err(error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use super::*;
    use crate::{ProviderError, ProviderErrorKind, ProviderId};

    #[test]
    fn retry_policy_uses_retryable_flag_and_attempt_limit() {
        let policy = RetryPolicy::new(3);
        let retryable = ProviderError::timeout("timed out");
        let non_retryable = ProviderError::invalid_request("bad request");

        assert!(policy.should_retry(1, &retryable));
        assert!(policy.should_retry(2, &retryable));
        assert!(!policy.should_retry(3, &retryable));
        assert!(!policy.should_retry(1, &non_retryable));
    }

    #[test]
    fn retry_policy_backoff_grows_and_caps() {
        let policy = RetryPolicy {
            max_attempts: 5,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_millis(250),
            backoff_multiplier: 2.0,
        };

        assert_eq!(policy.backoff_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.backoff_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.backoff_for_attempt(3), Duration::from_millis(250));
        assert_eq!(policy.backoff_for_attempt(4), Duration::from_millis(250));
    }

    #[derive(Default)]
    struct RecordingHooks {
        events: Mutex<Vec<String>>,
    }

    impl ProviderOperationHooks for RecordingHooks {
        fn on_attempt_start(&self, provider: ProviderId, operation: &str, attempt: u32) {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("start:{provider}:{operation}:{attempt}"));
        }

        fn on_retry_scheduled(
            &self,
            provider: ProviderId,
            operation: &str,
            attempt: u32,
            _delay: Duration,
            _error: &ProviderError,
        ) {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("retry:{provider}:{operation}:{attempt}"));
        }

        fn on_success(&self, provider: ProviderId, operation: &str, attempts: u32) {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("success:{provider}:{operation}:{attempts}"));
        }

        fn on_failure(
            &self,
            provider: ProviderId,
            operation: &str,
            attempts: u32,
            error: &ProviderError,
        ) {
            self.events
                .lock()
                .expect("events lock")
                .push(format!("failure:{provider}:{operation}:{attempts}:{:?}", error.kind));
        }
    }

    #[tokio::test]
    async fn execute_with_retry_retries_and_reports_hooks() {
        let policy = RetryPolicy::new(3);
        let hooks = RecordingHooks::default();
        let attempts = Arc::new(Mutex::new(0_u32));
        let sleeps = Arc::new(Mutex::new(Vec::new()));

        let result = execute_with_retry(
            ProviderId::OpenAi,
            "complete",
            &policy,
            &hooks,
            {
                let attempts = Arc::clone(&attempts);
                move |attempt| {
                    let attempts = Arc::clone(&attempts);
                    async move {
                        *attempts.lock().expect("attempts lock") = attempt;
                        if attempt < 3 {
                            Err(ProviderError::new(
                                ProviderErrorKind::Transport,
                                "temporary",
                                true,
                            ))
                        } else {
                            Ok("ok")
                        }
                    }
                }
            },
            {
                let sleeps = Arc::clone(&sleeps);
                move |delay| {
                    let sleeps = Arc::clone(&sleeps);
                    async move {
                        sleeps.lock().expect("sleep lock").push(delay);
                    }
                }
            },
        )
        .await;

        assert_eq!(result.expect("result should succeed"), "ok");
        assert_eq!(*attempts.lock().expect("attempts lock"), 3);
        assert_eq!(sleeps.lock().expect("sleep lock").len(), 2);

        let events = hooks.events.lock().expect("events lock").clone();
        assert!(events.contains(&"success:openai:complete:3".to_string()));
    }

    #[tokio::test]
    async fn execute_with_retry_stops_on_non_retryable_error() {
        let policy = RetryPolicy::new(5);
        let hooks = RecordingHooks::default();

        let result = execute_with_retry::<(), _, _, _, _>(
            ProviderId::OpenAi,
            "complete",
            &policy,
            &hooks,
            |_| async move { Err(ProviderError::invalid_request("bad input")) },
            |_| async move {},
        )
        .await;

        let error = result.expect_err("result should fail");
        assert_eq!(error.kind, ProviderErrorKind::InvalidRequest);
        let events = hooks.events.lock().expect("events lock").clone();
        assert!(events.iter().any(|item| item.contains("failure:openai:complete:1")));
    }
}
