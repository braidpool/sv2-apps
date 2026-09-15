use error::PoolErrorKind;
use pool_runtime::{Init, PoolRuntime};

pub mod braidpool;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use stratum_apps::bitcoin_core_sv2::CancellationToken;
use tokio::sync::Notify;
use tracing::info;

use braidpool_common::template::BraidpoolTemplate;

use crate::{braidpool::ShareBridgeReceiver, config::PoolConfig};

pub mod channel_manager;
pub mod config;
pub mod downstream;
pub mod error;
mod io_task;
#[cfg(feature = "monitoring")]
mod monitoring;
mod pool_runtime;
pub mod template_receiver;
pub mod utils;

#[derive(Debug, Clone)]
pub struct PoolSv2 {
    config: PoolConfig,
    cancellation_token: CancellationToken,
    shutdown_notify: Arc<Notify>,
    is_alive: Arc<AtomicBool>,
    /// In-process template channel for `BraidpoolIpc` mode.
    /// Wrapped in `Arc<Mutex<Option<...>>>` so `PoolSv2` remains `Clone`.
    braidpool_template_rx:
        Arc<std::sync::Mutex<Option<tokio::sync::mpsc::Receiver<BraidpoolTemplate>>>>,
}

#[cfg_attr(not(test), hotpath::measure_all)]
impl PoolSv2 {
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            cancellation_token: CancellationToken::new(),
            shutdown_notify: Arc::new(Notify::new()),
            is_alive: Arc::new(AtomicBool::new(true)),
            braidpool_template_rx: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Wires the in-process template channel for `BraidpoolIpc` mode.
    ///
    /// Must be called before [`start`] when `template_provider_type = BraidpoolIpc`.
    /// The receiver is consumed once during bootstrap; calling `start` a second
    /// time after the channel has been taken will fail with a configuration error.
    pub fn with_braidpool_template_rx(
        self,
        rx: tokio::sync::mpsc::Receiver<BraidpoolTemplate>,
    ) -> Self {
        self.braidpool_template_rx
            .lock()
            .expect("braidpool_template_rx mutex poisoned")
            .replace(rx);
        self
    }

    pub(crate) fn take_braidpool_template_rx(
        &self,
    ) -> Option<tokio::sync::mpsc::Receiver<BraidpoolTemplate>> {
        self.braidpool_template_rx
            .lock()
            .expect("braidpool_template_rx mutex poisoned")
            .take()
    }

    /// Starts the Pool server and blocks asynchronously on the `PoolRuntime`.
    ///
    /// The startup and execution sequence follows:
    /// 1. **Initialize:** Sets up the pool runtime state machine.
    /// 2. **Bootstrap:** Configures internal channels, starts the Job Declarator Server (JDS),
    ///    connects to the Template Provider, and initializes the Channel Manager.
    /// 3. **Run & Block:** Spawns active background loops/servers and blocks the caller while
    ///    awaiting the runtime's shutdown signal (e.g., Ctrl+C or program cancellation).
    /// 4. **Teardown:** Performs a coordinated graceful cleanup of all services and tasks,
    ///    remaining blocked until all sub-services have exited.
    ///
    /// If any error occurs during bootstrapping, `start` receives the partially initialized
    /// runtime, gracefully shuts it down, and then returns the error.
    ///
    /// When `share_rx_notify` is `Some`, the [`ShareBridgeReceiver`] is sent over it once
    /// the channel manager is bootstrapped and the pool is ready to process shares. The
    /// caller can then forward validated shares to `propagate_valid_bead` without waiting
    /// for the pool to shut down.
    pub async fn start(
        &self,
        share_rx_notify: Option<tokio::sync::oneshot::Sender<ShareBridgeReceiver>>,
    ) -> Result<(), PoolErrorKind> {
        let runtime = match PoolRuntime::<Init>::new(self.clone()) {
            Ok(runtime) => runtime,
            Err(err) => {
                self.mark_stopped();
                return Err(err);
            }
        };

        let mut runtime = match runtime.bootstrap().await {
            Ok(runtime) => runtime,
            Err(err) => {
                let (err, runtime) = err.into_parts();
                runtime.shutdown().await;
                return Err(err);
            }
        };

        // Deliver the share bridge receiver to the caller before blocking on shutdown,
        // so they can start consuming shares without waiting for the pool to exit.
        if let Some(tx) = share_rx_notify {
            if let Some(rx) = runtime.take_share_bridge_receiver() {
                let _ = tx.send(rx);
            }
        }

        runtime.wait_for_shutdown().await;
        runtime.shutdown().await;

        Ok(())
    }

    /// Marks the pool as stopped and releases anyone blocked on [`PoolSv2::shutdown`].
    ///
    /// Called by [`PoolRuntime::shutdown`] once teardown completes, and directly by
    /// [`PoolSv2::start`] on the failure path where no runtime exists to tear down.
    fn mark_stopped(&self) {
        self.is_alive.store(false, Ordering::Release);
        self.cancellation_token.cancel();
        self.shutdown_notify.notify_waiters();
    }

    pub async fn shutdown(&self) {
        if !self.is_alive.load(Ordering::Acquire) {
            return;
        }
        // The Notified future is guaranteed to receive wakeups from notify_waiters()
        // as soon as it has been created, even if it has not yet been polled.
        let notified = self.shutdown_notify.notified();
        self.cancellation_token.cancel();
        notified.await;
    }
}

impl Drop for PoolSv2 {
    fn drop(&mut self) {
        info!("PoolSv2 dropped");
        self.cancellation_token.cancel();
    }
}
