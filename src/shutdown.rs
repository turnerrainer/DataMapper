//! Graceful shutdown signal.
//!
//! h2ck.me v1 NEXT-TASKS.md §T-18 — pre-fix, DataMapper's process
//! exited abruptly on `SIGTERM` (the default kubelet /
//! docker-compose stop signal), tearing down TCP connections
//! mid-response and burning a shot on any client that had already
//! sent its request bytes. Wire an axum `with_graceful_shutdown` so
//! `axum::serve` receives a future that resolves on the first
//! `SIGTERM` / `SIGINT` / `SIGHUP`, then stops accepting new
//! connections and lets already-dispatched requests finish.
//!
//! Signals handled (Unix):
//! - `SIGTERM` — the orchestrator's "please stop" (kubelet, systemd,
//!   docker-compose down).
//! - `SIGINT`  — interactive `Ctrl-C` under `docker run -it`.
//! - `SIGHUP` — some log-rotation / config-reload workflows send
//!   this expecting graceful reload; DataMapper treats it as a
//!   stop signal (there is no reload path today; a fresh boot is
//!   the reload).
//!
//! On non-Unix targets the signal listener is a `SIGINT` (Ctrl-C)
//! only wire — Windows-service management sends different
//! primitives that are out of scope for a container-shipped
//! service.

use tokio::sync::oneshot;

/// Wait for the first shutdown signal to fire.
///
/// Resolves as soon as *any* of `SIGTERM` / `SIGINT` / `SIGHUP`
/// (Unix) or Ctrl-C (all platforms) is observed. Never resolves
/// spontaneously — the future is pending until the OS delivers a
/// signal or the process is otherwise killed.
///
/// Idempotent within a process: multiple concurrent awaiters each
/// see the first signal. Signal handlers are installed lazily by
/// `tokio::signal::unix::signal(...)` — the runtime keeps them
/// alive for the lifetime of the returned stream, which we hold
/// on the stack of this future.
///
/// Consumed by `axum::serve(...).with_graceful_shutdown(...)`.
pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let term = async {
            match signal(SignalKind::terminate()) {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(e) => {
                    tracing::warn!(
                        "failed to install SIGTERM handler: {e} — service will exit on Ctrl-C only"
                    );
                    std::future::pending::<()>().await;
                }
            }
        };
        let int = async {
            if let Err(e) = tokio::signal::ctrl_c().await {
                tracing::warn!("failed to install SIGINT handler: {e}");
                std::future::pending::<()>().await;
            }
        };
        let hup = async {
            match signal(SignalKind::hangup()) {
                Ok(mut s) => {
                    s.recv().await;
                }
                Err(e) => {
                    tracing::warn!("failed to install SIGHUP handler: {e}");
                    std::future::pending::<()>().await;
                }
            }
        };
        tokio::select! {
            _ = term => tracing::info!("received SIGTERM — draining"),
            _ = int  => tracing::info!("received SIGINT — draining"),
            _ = hup  => tracing::info!("received SIGHUP — draining"),
        }
    }
    #[cfg(not(unix))]
    {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!("failed to install Ctrl-C handler: {e}");
            std::future::pending::<()>().await;
        }
        tracing::info!("received Ctrl-C — draining");
    }
}

/// Test seam for the shutdown wiring — resolve when the given
/// channel fires. Kept out of the production `shutdown_signal`
/// path so tests can exercise the composition without relying on
/// real signal delivery.
pub async fn shutdown_from_channel(rx: oneshot::Receiver<()>) {
    let _ = rx.await;
}
