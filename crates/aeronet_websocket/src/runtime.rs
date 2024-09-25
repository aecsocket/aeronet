use {bevy_ecs::prelude::*, std::future::Future};

/// Provides a platform-agnostic way to spawn futures for driving the
/// WebSocket IO layer.
///
/// Using WebSocket sessions requires spawning tasks on an async runtime.
/// However, which runtime to use exactly, and how that runtime is provided, is
/// target-dependent. This resource exists to provide a platform-agnostic way of
/// spawning these tasks.
///
/// # Platforms
///
/// ## Native
///
/// On a native target, this holds a handle to a `tokio` runtime, because
/// `tokio-tungstenite` only supports this async runtime.
///
/// Use the [`Default`] impl to create and leak a new `tokio` runtime, and that
/// as the [`WebSocketRuntime`] handle.
///
/// If you already have a runtime handle, you can use
/// `WebSocketRuntime::from(handle)` to create a runtime from that handle.
///
/// ## WASM
///
/// On a WASM target, this uses `wasm-bindgen-futures` to spawn the future via
/// `wasm-bindgen`.
///
/// Use the [`Default`] impl to create a new [`WebSocketRuntime`] on WASM.
#[derive(Debug, Clone, Resource)]
pub struct WebSocketRuntime {
    #[cfg(target_family = "wasm")]
    _priv: (),
    #[cfg(not(target_family = "wasm"))]
    handle: tokio::runtime::Handle,
}

#[cfg(target_family = "wasm")]
mod maybe {
    pub trait Send {}
    impl<T> Send for T {}
}

#[cfg(not(target_family = "wasm"))]
mod maybe {
    pub trait Send: core::marker::Send {}
    impl<T: core::marker::Send> Send for T {}
}

#[cfg_attr(
    target_family = "wasm",
    allow(
        clippy::derivable_impls,
        reason = "constructor has conditional cfg logic"
    )
)]
impl Default for WebSocketRuntime {
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
impl From<tokio::runtime::Handle> for WebSocketRuntime {
    fn from(value: tokio::runtime::Handle) -> Self {
        Self { handle: value }
    }
}

impl WebSocketRuntime {
    /// Spawns a future on the task runtime `self`.
    ///
    /// If you are already in a task context, use [`WebSocketRuntime::spawn`]
    /// to avoid having to pass around [`WebSocketRuntime`].
    pub fn spawn_on_self<F>(&self, future: F)
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
}
