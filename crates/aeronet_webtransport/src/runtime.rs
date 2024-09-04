//! See [`WebTransportRuntime`].

use {
    bevy_ecs::prelude::*,
    std::{future::Future, time::Duration},
    xwt_core::utils::maybe,
};

#[derive(Debug, Clone, Resource)]
pub struct WebTransportRuntime {
    #[cfg(target_family = "wasm")]
    _priv: (),
    #[cfg(not(target_family = "wasm"))]
    handle: tokio::runtime::Handle,
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
                handle: runtime.handle().clone(),
            }
        }
    }
}

#[cfg(not(target_family = "wasm"))]
impl From<tokio::runtime::Handle> for WebTransportRuntime {
    fn from(value: tokio::runtime::Handle) -> Self {
        Self { handle: value }
    }
}

impl WebTransportRuntime {
    /// Gets a handle to the underlying [`tokio`] runtime.
    ///
    /// This function only exists on platforms which use [`tokio`] as their
    /// underlying runtime (i.e. not on WASM).
    #[cfg(not(target_family = "wasm"))]
    #[must_use]
    pub fn handle(&self) -> tokio::runtime::Handle {
        self.handle.clone()
    }

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
            self.handle.spawn(future);
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
