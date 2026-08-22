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
use super::signal::{Signal, create_stored};

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
/// `Copy`, like [`Signal`]: the channel lives in the reactive
/// arena and the handle is just its id, so it can be dropped into as many
/// closures as needed without a clone. Handles of services, signals and
/// callbacks being `Copy` is what keeps a struct that groups them `Copy` too.
///
/// Main-thread only (`!Send`). To send commands from a background task, take
/// the [`sender()`](Service::sender) — the same split as
/// [`RwSignal::writer`](super::RwSignal::writer).
pub struct Service<Cmd> {
    sender: Signal<mpsc::UnboundedSender<Cmd>>,
}

// Manual impls: the derives would demand `Cmd: Clone`/`Cmd: Copy`, but the
// handle is an arena id whatever the command type is.
impl<Cmd> Clone for Service<Cmd> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Cmd> Copy for Service<Cmd> {}

impl<Cmd: 'static> Service<Cmd> {
    /// Send a command to the service.
    ///
    /// Returns silently if the service has stopped. Sending after the owning
    /// scope was disposed logs a warning and does nothing — the task it would
    /// reach was aborted with that scope.
    pub fn send(&self, cmd: Cmd) {
        self.sender.with_untracked(|tx| {
            let _ = tx.send(cmd);
        });
    }

    /// The `Send` half of this handle, for driving the service from a
    /// background task. The counterpart of `RwSignal::writer()`.
    pub fn sender(&self) -> mpsc::UnboundedSender<Cmd> {
        self.sender.with_untracked(|tx| tx.clone())
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
/// A task with nothing to command is [`create_task`], which has no receiver
/// and no turbofish.
pub fn create_service<Cmd, F, Fut>(f: F) -> Service<Cmd>
where
    Cmd: Send + 'static,
    F: FnOnce(mpsc::UnboundedReceiver<Cmd>, ServiceContext) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let (tx, rx) = mpsc::unbounded_channel();
    // The channel lives in the arena so the handle can be Copy
    let sender = create_stored(tx);
    spawn_owned(|ctx| f(rx, ctx));
    Service { sender }
}

/// Spawn a future on the service runtime, owned by the current scope.
///
/// The half both [`create_service`] and [`create_task`] need: the context, the
/// spawn, and the cleanup that stops it. Setting `is_running` to false allows a
/// graceful shutdown, while `abort()` cancels the task at its next `.await` for
/// fast cleanup — which is what keeps a `WriteSignal` from writing after an App
/// restart.
/// Build the future on *this* thread and hand only the future to the runtime.
///
/// Which is why neither [`create_service`] nor [`create_task`] asks its factory
/// to be `Send`: the factory runs here, so it may read whatever the caller has —
/// a `Signal`, an `Rc` — as long as only owned data crosses into the `async`
/// block. Both of them used to demand it anyway, which rejected exactly the
/// idiom their own documentation shows.
fn spawn_owned<F, Fut>(f: F)
where
    F: FnOnce(ServiceContext) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let running = Arc::new(AtomicBool::new(true));
    let running_for_cleanup = running.clone();
    let handle = service_runtime().spawn(f(ServiceContext { running }));

    on_cleanup(move || {
        running_for_cleanup.store(false, Ordering::SeqCst);
        handle.abort();
    });
}

/// Create a background task tied to the current Owner.
///
/// The half of [`create_service`] that only pushes: no commands, so no
/// receiver, no command type to name, no channel allocated and no handle to
/// keep. It is aborted when the scope that created it is disposed, exactly as a
/// service is.
///
/// ```ignore
/// let time = create_signal(String::new());
/// let time_w = time.writer();
///
/// create_task(move |ctx| async move {
///     while ctx.is_running() {
///         time_w.set(chrono::Local::now().format("%H:%M").to_string());
///         tokio::time::sleep(Duration::from_secs(1)).await;
///     }
/// });
/// ```
pub fn create_task<F, Fut>(f: F)
where
    F: FnOnce(ServiceContext) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    // Not `create_service::<(), _, _>`: that would allocate a channel whose
    // receiver is dropped at the first poll, and park its sender in an arena
    // slot held until the scope is disposed. A status bar's dozen push-only
    // tasks are a dozen dead channels.
    spawn_owned(f);
}

/// Get a runtime handle for spawning service tasks.
///
/// If the caller already runs inside a tokio runtime (e.g. `#[tokio::main]`),
/// that runtime is used. Otherwise a small background runtime is created
/// lazily on a dedicated thread — previously `create_service` simply called
/// `tokio::spawn` and panicked in the documented plain-`fn main()` setup.
fn service_runtime() -> tokio::runtime::Handle {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return handle;
    }

    static HANDLE: std::sync::OnceLock<tokio::runtime::Handle> = std::sync::OnceLock::new();
    HANDLE
        .get_or_init(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("failed to build guido service runtime");
            let handle = runtime.handle().clone();
            std::thread::Builder::new()
                .name("guido-services".into())
                .spawn(move || {
                    // Drive the runtime forever; tasks spawned through the
                    // handle run on this thread.
                    runtime.block_on(std::future::pending::<()>());
                })
                .expect("failed to spawn guido service thread");
            handle
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::owner::{dispose_owner_now, with_owner};
    use std::sync::atomic::AtomicI32;
    use std::time::Duration;

    /// Regression test: create_service must work WITHOUT an ambient tokio
    /// runtime (plain `fn main()`, the documented default setup). It
    /// previously called tokio::spawn directly and panicked.
    #[test]
    fn test_service_without_ambient_runtime() {
        let received = Arc::new(AtomicI32::new(0));
        let received_clone = received.clone();

        let (service, owner_id) = with_owner(|| {
            create_service::<i32, _, _>(move |mut rx, _ctx| async move {
                while let Some(v) = rx.recv().await {
                    received_clone.fetch_add(v, Ordering::SeqCst);
                }
            })
        });

        service.send(5);

        // The task runs on the lazily-created background thread
        let mut waited = 0;
        while received.load(Ordering::SeqCst) == 0 && waited < 2000 {
            std::thread::sleep(Duration::from_millis(10));
            waited += 10;
        }
        assert_eq!(received.load(Ordering::SeqCst), 5);

        dispose_owner_now(owner_id);
    }

    /// A task has nothing to receive, so it must not build the machinery for
    /// receiving. Going through `create_service::<(), _, _>` allocated a channel
    /// whose receiver died at the first poll and parked its sender in an arena
    /// slot held until disposal — a dozen push-only tasks, a dozen dead
    /// channels.
    #[test]
    fn a_task_allocates_no_channel() {
        let before = crate::reactive::storage::slot_count();
        let (_, owner_id) = with_owner(|| {
            create_task(|ctx| async move {
                let _ = ctx.is_running();
            });
        });
        assert_eq!(
            crate::reactive::storage::slot_count(),
            before,
            "a task must claim no arena slot"
        );
        dispose_owner_now(owner_id);

        // The service, which does have something to receive, still claims one.
        let before = crate::reactive::storage::slot_count();
        let (_, owner_id) = with_owner(|| {
            create_service::<i32, _, _>(|mut rx, _ctx| async move {
                let _ = rx.recv().await;
            })
        });
        assert!(crate::reactive::storage::slot_count() > before);
        dispose_owner_now(owner_id);
    }

    #[tokio::test]
    async fn test_service_stops_on_cleanup() {
        let counter = Arc::new(AtomicI32::new(0));
        let counter_clone = counter.clone();

        let (_, owner_id) = with_owner(|| {
            create_task(move |ctx| async move {
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
        dispose_owner_now(owner_id);

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
        dispose_owner_now(owner_id);
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
        let service2 = service;
        service.send(5);
        service2.send(7);

        tokio::time::sleep(Duration::from_millis(50)).await;

        assert_eq!(received.load(Ordering::SeqCst), 12);

        dispose_owner_now(owner_id);
    }
}

#[cfg(test)]
mod factory_bounds {
    use super::*;

    /// Compile-time only: the factory runs on the calling thread, so it may
    /// capture something that could never cross to the runtime. Nothing calls
    /// this — spawning would want a scope and a runtime, and the claim is about
    /// what type-checks.
    #[allow(dead_code)]
    fn a_factory_may_read_a_non_send_value() {
        let local = std::rc::Rc::new(7u8);
        create_task(move |_ctx| {
            let owned = *local;
            async move {
                let _ = owned;
            }
        });

        let local = std::rc::Rc::new(9u8);
        let _service = create_service::<(), _, _>(move |_rx, _ctx| {
            let owned = *local;
            async move {
                let _ = owned;
            }
        });
    }
}
