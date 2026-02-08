//! Background service system for component-scoped async tasks.
//!
//! This module provides a convenient API for spawning background services
//! that are automatically cleaned up when the component unmounts.
//!
//! # Example
//!
//! ```ignore
//! let service = create_service(move |mut rx, ctx| async move {
//!     while ctx.is_running() {
//!         if let Some(cmd) = rx.recv().await {
//!             // handle command
//!         }
//!         // update signals
//!     }
//! });
//!
//! service.send(MyCommand::DoSomething);
//! ```

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::mpsc;

use super::on_cleanup;

/// Context passed to the service function.
///
/// Use `is_running()` to check if the service should continue running.
/// When the component unmounts, `is_running()` will return `false`.
pub struct ServiceContext {
    running: Arc<AtomicBool>,
}

impl ServiceContext {
    /// Returns `true` if the service should continue running.
    ///
    /// Returns `false` when the component has been unmounted and the
    /// service should terminate.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

/// Handle to a background service for sending commands.
///
/// Clone this handle to send commands from multiple places.
#[derive(Clone)]
pub struct Service<Cmd> {
    sender: mpsc::UnboundedSender<Cmd>,
}

impl<Cmd> Service<Cmd> {
    /// Send a command to the service.
    ///
    /// Returns silently if the service has stopped.
    pub fn send(&self, cmd: Cmd) {
        let _ = self.sender.send(cmd);
    }
}

/// Create a background service tied to the current Owner.
///
/// The service task runs until the component unmounts, at which point
/// `ctx.is_running()` will return `false`. The service function receives:
///
/// - `rx`: A receiver for commands sent via `service.send()`
/// - `ctx`: A context with `is_running()` to check if the service should continue
///
/// # Example: Bidirectional Service
///
/// ```ignore
/// enum Cmd {
///     SwitchWorkspace(i32),
/// }
///
/// let active = create_signal(1i32);
/// let service = create_service(move |mut rx, ctx| async move {
///     loop {
///         tokio::select! {
///             Some(cmd) = rx.recv() => {
///                 match cmd {
///                     Cmd::SwitchWorkspace(id) => {
///                         // perform action
///                     }
///                 }
///             }
///             _ = tokio::time::sleep(Duration::from_millis(50)) => {
///                 if !ctx.is_running() { break; }
///             }
///         }
///     }
/// });
///
/// // Send commands from UI
/// service.send(Cmd::SwitchWorkspace(2));
/// ```
///
/// # Example: Read-Only Service
///
/// For services that only push data to signals (no commands), use `()` as
/// the command type and ignore the receiver:
///
/// ```ignore
/// let time = create_signal(String::new());
/// let time_w = time.writer();
///
/// let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
///     while ctx.is_running() {
///         time_w.set(chrono::Local::now().format("%H:%M").to_string());
///         tokio::time::sleep(Duration::from_secs(1)).await;
///     }
/// });
/// ```
pub fn create_service<Cmd, F, Fut>(f: F) -> Service<Cmd>
where
    Cmd: Send + 'static,
    F: FnOnce(mpsc::UnboundedReceiver<Cmd>, ServiceContext) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = mpsc::unbounded_channel();
    let running = Arc::new(AtomicBool::new(true));
    let running_for_cleanup = running.clone();

    // Register cleanup to stop the task when component unmounts
    on_cleanup(move || {
        running_for_cleanup.store(false, Ordering::SeqCst);
    });

    let ctx = ServiceContext { running };
    tokio::spawn(f(rx, ctx));

    Service { sender: tx }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::owner::{dispose_owner, with_owner};
    use std::sync::atomic::AtomicI32;
    use std::time::Duration;

    #[tokio::test]
    async fn test_service_stops_on_cleanup() {
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        let (_, owner_id) = with_owner(|| {
            let _ = create_service::<(), _, _>(move |_rx, ctx| async move {
                while ctx.is_running() {
                    counter_clone.fetch_add(1, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            });
        });

        // Let the service run a bit
        tokio::time::sleep(Duration::from_millis(50)).await;
        let count_before = counter.load(Ordering::SeqCst);
        assert!(count_before > 0, "Service should have run at least once");

        // Dispose the owner
        dispose_owner(owner_id);

        // Wait for service to notice and stop
        tokio::time::sleep(Duration::from_millis(50)).await;
        let count_after = counter.load(Ordering::SeqCst);

        // Wait a bit more and check count doesn't increase
        tokio::time::sleep(Duration::from_millis(50)).await;
        let count_final = counter.load(Ordering::SeqCst);

        assert_eq!(
            count_after, count_final,
            "Service should have stopped after owner disposal"
        );
    }

    #[tokio::test]
    async fn test_service_receives_commands() {
        let received = Arc::new(AtomicI32::new(0));
        let received_clone = received.clone();

        let (service, owner_id) = with_owner(|| {
            create_service::<i32, _, _>(move |mut rx, ctx| async move {
                while ctx.is_running() {
                    match rx.try_recv() {
                        Ok(cmd) => {
                            received_clone.fetch_add(cmd, Ordering::SeqCst);
                        }
                        Err(_) => {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    }
                }
            })
        });

        // Send some commands
        service.send(10);
        service.send(20);
        service.send(30);

        // Wait for processing
        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(received.load(Ordering::SeqCst), 60);

        // Cleanup
        dispose_owner(owner_id);
    }

    #[tokio::test]
    async fn test_service_handle_is_clone() {
        let received = Arc::new(AtomicI32::new(0));
        let received_clone = received.clone();

        let (service, owner_id) = with_owner(|| {
            create_service::<i32, _, _>(move |mut rx, ctx| async move {
                while ctx.is_running() {
                    match rx.try_recv() {
                        Ok(cmd) => {
                            received_clone.fetch_add(cmd, Ordering::SeqCst);
                        }
                        Err(_) => {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    }
                }
            })
        });

        // Clone and send from different handles
        let service2 = service.clone();
        service.send(5);
        service2.send(7);

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(received.load(Ordering::SeqCst), 12);

        dispose_owner(owner_id);
    }
}
