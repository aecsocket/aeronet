//! See [`WebTransportRuntime`].

use std::{future::Future, time::Duration};

use xwt_core::utils::maybe;

/// Provides a platform-agnostic way of spawning futures required to drive a
/// WebTransport endpoint.
///
/// [`WebTransportClient::connect`] and [`WebTransportServer::open`] both return
/// [`Future`]s which must be spawned on an async runtime. However, which
/// runtime to use exactly (and how that runtime is provided) is
/// target-dependent. This type exists to provide a platform-agnostic way of
/// running those futures on a runtime.
///
/// This is also used internally, as clients and servers may need to spawn their
/// own tasks for e.g. the sending and receiving halves of a session.
///
/// On a native target, this holds a handle to a `tokio` runtime, because
/// `wtransport` currently only supports this async runtime. The [`Default`]
/// impl will create and leak a new `tokio` runtime, and store a handle to this
/// leaked runtime.
///
/// On a WASM target, this uses `wasm-bindgen-futures` to spawn the future via
/// `wasm-bindgen`.
///
/// If using Bevy, you can use this as a resource in your systems.
///
/// [`WebTransportClient::connect`]: crate::client::WebTransportClient::connect
/// [`WebTransportServer::open`]: crate::server::WebTransportServer::open
#[derive(Debug, Clone)]
#[cfg_attr(feature = "bevy", derive(bevy_ecs::prelude::Resource))]
pub struct WebTransportRuntime {
    #[cfg(target_family = "wasm")]
    _priv: (),
    #[cfg(not(target_family = "wasm"))]
    runtime: tokio::runtime::Handle,
}

#[allow(clippy::derivable_impls)] // no it can't because conditional cfg logic
impl Default for WebTransportRuntime {
    fn default() -> Self {
        #[cfg(target_family = "wasm")]
        {
            Self { _priv: () }
        }
        #[cfg(not(target_family = "wasm"))]
        {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to create tokio runtime");
            let runtime = Box::leak(Box::new(runtime));
            Self {
                runtime: runtime.handle().clone(),
            }
        }
    }
}

impl WebTransportRuntime {
    /// Spawns a future on the task runtime.
    pub fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + maybe::Send + 'static,
    {
        #[cfg(target_family = "wasm")]
        {
            wasm_bindgen_futures::spawn_local(future);
        }
        #[cfg(not(target_family = "wasm"))]
        {
            self.runtime.spawn(future);
        }
    }

    /// Pauses execution for the given duration.
    pub async fn sleep(&self, duration: Duration) {
        #[cfg(target_family = "wasm")]
        {
            gloo_timers::future::sleep(duration).await;
        }
        #[cfg(not(target_family = "wasm"))]
        {
            tokio::time::sleep(duration).await;
        }
    }
}
