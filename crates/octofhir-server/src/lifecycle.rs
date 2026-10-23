//! Server-owned background tasks, including routers embedded in another service.

use std::{future::Future, sync::Arc};

use tokio_util::{sync::CancellationToken, task::TaskTracker};

#[derive(Clone, Default)]
pub(crate) struct BackgroundRuntime {
    pub(crate) cancellation: CancellationToken,
    tasks: TaskTracker,
}

/// Only the router owns this guard. Workers hold the runtime, never the guard,
/// so dropping an embedded router also stops its workers.
pub(crate) struct BackgroundOwner(BackgroundRuntime);

impl Drop for BackgroundOwner {
    fn drop(&mut self) {
        self.0.stop();
    }
}

impl BackgroundRuntime {
    pub(crate) fn owner(&self) -> Arc<BackgroundOwner> {
        Arc::new(BackgroundOwner(self.clone()))
    }

    pub(crate) fn stop(&self) {
        self.cancellation.cancel();
        self.tasks.close();
    }

    pub(crate) async fn wait(&self) {
        self.tasks.wait().await;
    }

    /// Long-running listeners and periodic maintenance have no write checkpoint
    /// to drain. Drop their future on shutdown, including any checked-out SQL.
    pub(crate) fn spawn<F>(&self, task: F) -> tokio::task::JoinHandle<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let cancellation = self.cancellation.clone();
        self.spawn_tracked(async move {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {},
                _ = task => {},
            }
        })
    }

    /// Workers handle cancellation themselves to drop the resource transaction
    /// before releasing their fenced claim. Shutdown still awaits their exit.
    pub(crate) fn spawn_tracked<F>(&self, task: F) -> tokio::task::JoinHandle<()>
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.tasks.spawn(task)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn last_router_owner_stops_and_awaits_listener_destructors() {
        struct NotifyDrop(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for NotifyDrop {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }
        let runtime = BackgroundRuntime::default();
        let owner = runtime.owner();
        let other = owner.clone();
        let (dropped, receive) = tokio::sync::oneshot::channel();
        runtime.spawn(async move {
            let _guard = NotifyDrop(Some(dropped));
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        drop(owner);
        assert!(!runtime.cancellation.is_cancelled());
        drop(other);
        tokio::time::timeout(std::time::Duration::from_secs(1), runtime.wait())
            .await
            .unwrap();
        receive.await.unwrap();
        assert!(runtime.tasks.is_empty());
    }
}
