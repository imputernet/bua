// runtime/src/promise/future.rs
//
// JsPromiseFuture: a Rust Future that resolves when a JS Promise resolves.
//
// This is the inverse of PromiseBridge::spawn_task —
// it allows Rust code to await a JS-originated Promise.
//
// Use case: Bua.agent.spawn() returns a JS Promise.
//           The scheduler awaits its Rust Future.

use crate::ffi::value::{JsException, JsValue};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

/// Internal shared state between the Future and the completion callback.
#[derive(Debug)]
struct FutureState {
    result: Option<Result<JsValue, JsException>>,
    waker: Option<Waker>,
}

/// A Rust Future that waits for a JS Promise to resolve or reject.
///
/// The JS thread calls `resolver.complete(value)` or `resolver.fail(ex)`
/// when the Promise settles. This wakes the Tokio task.
#[derive(Debug)]
pub struct JsPromiseFuture {
    state: Arc<Mutex<FutureState>>,
}

/// The JS-thread side of a JsPromiseFuture.
/// Passed to the resolution callback.
#[derive(Clone, Debug)]
pub struct JsPromiseResolver {
    state: Arc<Mutex<FutureState>>,
}

impl JsPromiseFuture {
    /// Create a linked (future, resolver) pair.
    pub fn new() -> (Self, JsPromiseResolver) {
        let state = Arc::new(Mutex::new(FutureState {
            result: None,
            waker: None,
        }));
        (
            Self {
                state: state.clone(),
            },
            JsPromiseResolver { state },
        )
    }
}

impl JsPromiseResolver {
    /// Resolve the future with a value. Called from JS thread or Tokio task.
    pub fn complete(&self, value: JsValue) {
        let mut state = self.state.lock().unwrap();
        state.result = Some(Ok(value));
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
    }

    /// Reject the future with an exception.
    pub fn fail(&self, ex: JsException) {
        let mut state = self.state.lock().unwrap();
        state.result = Some(Err(ex));
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
    }

    /// True if the future has already been settled.
    pub fn is_settled(&self) -> bool {
        self.state.lock().unwrap().result.is_some()
    }
}

impl Future for JsPromiseFuture {
    type Output = Result<JsValue, JsException>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut state = self.state.lock().unwrap();
        if let Some(result) = state.result.take() {
            Poll::Ready(result)
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::Duration;

    #[tokio::test]
    async fn resolve_wakes_future() {
        let (fut, resolver) = JsPromiseFuture::new();
        assert!(!resolver.is_settled());

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            resolver.complete(JsValue::String("done".into()));
        });

        let result = fut.await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().as_str(), Some("done"));
    }

    #[tokio::test]
    async fn reject_wakes_future() {
        let (fut, resolver) = JsPromiseFuture::new();

        tokio::spawn(async move {
            resolver.fail(JsException::new("bad input"));
        });

        let result = fut.await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().message, "bad input");
    }

    #[tokio::test]
    async fn already_resolved_polls_immediately() {
        let (fut, resolver) = JsPromiseFuture::new();
        resolver.complete(JsValue::Bool(true));

        // Should resolve without any delay
        let result = tokio::time::timeout(Duration::from_millis(1), fut)
            .await
            .expect("should resolve immediately");
        assert!(result.is_ok());
    }
}
