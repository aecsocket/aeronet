#![cfg_attr(docsrs_aeronet, feature(doc_cfg))]
#![doc = include_str!("../README.md")]
#![cfg_attr(
    target_family = "wasm",
    expect(
        clippy::future_not_send,
        reason = "`Send`, `Sync` are not used on WASM"
    )
)]

extern crate alloc;

use {
    bevy_ecs::prelude::*,
    core::{future::Future, time::Duration},
};

/// Provides a platform-agnostic way to spawn futures for driving an `aeronet`
/// IO layer.
///
/// Using async IO session implementations requires spawning tasks on
/// an async runtime. However, which runtime to use exactly, and how that
/// runtime is provided, is target-dependent. This resource exists to provide a
/// platform-agnostic way of spawning these tasks.
///
/// # Platforms
///
/// ## Native
///
/// On a native target, this holds a handle to a `tokio` runtime, because the
/// various IO layers currently use this async runtime.
///
/// Use the [`FromWorld`] impl to create and leak a new `tokio` runtime, and use
/// that as the [`TokioRuntime`] handle.
///
/// If you already have a runtime handle, you can use
/// `TokioRuntime::from(handle)` to create a runtime from that handle.
///
/// ## WASM
///
/// On a WASM target, this uses `wasm-bindgen-futures` to spawn the future via
/// `wasm-bindgen`.
///
/// Use the [`FromWorld`] impl to create a new [`TokioRuntime`] on WASM.
#[derive(Debug, Clone, Resource)]
pub struct TokioRuntime {
    #[cfg(target_family = "wasm")]
    _priv: (),
    #[cfg(not(target_family = "wasm"))]
    inner: RuntimeInner,
}

/// How a [`TokioRuntime`] is provided on native targets.
///
/// # Availability
///
/// Only available on non-WASM targets.
#[cfg(not(target_family = "wasm"))]
#[derive(Debug, Clone)]
enum RuntimeInner {
    /// The runtime is owned by an external component, and we only hold a handle
    /// to it.
    Handle(tokio::runtime::Handle),
    /// We own the runtime ourselves.
    Runtime(alloc::sync::Arc<tokio::runtime::Runtime>),
}

#[cfg(target_family = "wasm")]
mod maybe {
    /// Marker trait which is implemented for all types on WASM, since `Send`
    /// is not enforced there.
    ///
    /// # Availability
    ///
    /// Only available on WASM targets.
    pub trait Send {}
    impl<T> Send for T {}
}

#[cfg(not(target_family = "wasm"))]
mod maybe {
    /// Marker trait which is implemented for all `Send` types on native
    /// targets.
    ///
    /// # Availability
    ///
    /// Only available on native targets.
    pub trait Send: core::marker::Send {}
    impl<T: core::marker::Send> Send for T {}
}

impl TokioRuntime {
    /// Creates a new runtime, available on WASM targets.
    #[cfg(target_family = "wasm")]
    #[must_use]
    pub const fn new_wasm() -> Self {
        Self { _priv: () }
    }
}

impl FromWorld for TokioRuntime {
    fn from_world(_: &mut World) -> Self {
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
            Self::from(runtime)
        }
    }
}

#[cfg(not(target_family = "wasm"))]
impl From<tokio::runtime::Handle> for TokioRuntime {
    fn from(value: tokio::runtime::Handle) -> Self {
        Self {
            inner: RuntimeInner::Handle(value),
        }
    }
}

#[cfg(not(target_family = "wasm"))]
impl From<alloc::sync::Arc<tokio::runtime::Runtime>> for TokioRuntime {
    fn from(value: alloc::sync::Arc<tokio::runtime::Runtime>) -> Self {
        Self {
            inner: RuntimeInner::Runtime(value),
        }
    }
}

#[cfg(not(target_family = "wasm"))]
impl From<tokio::runtime::Runtime> for TokioRuntime {
    fn from(value: tokio::runtime::Runtime) -> Self {
        Self {
            inner: RuntimeInner::Runtime(alloc::sync::Arc::new(value)),
        }
    }
}

impl TokioRuntime {
    /// Spawns a future on the task runtime `self`.
    ///
    /// If you are already in a task context, use [`TokioRuntime::spawn`] to
    /// avoid having to pass around [`TokioRuntime`].
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
            match &self.inner {
                RuntimeInner::Handle(handle) => handle.spawn(future),
                RuntimeInner::Runtime(runtime) => runtime.spawn(future),
            };
        }
    }

    /// Spawns a future on the task runtime running on this thread.
    ///
    /// You must call this from a context where you are already running a task
    /// on the reactor.
    pub fn spawn<F>(future: F)
    where
        F: Future<Output = ()> + maybe::Send + 'static,
    {
        #[cfg(target_family = "wasm")]
        {
            wasm_bindgen_futures::spawn_local(future);
        }

        #[cfg(not(target_family = "wasm"))]
        {
            tokio::spawn(future);
        }
    }

    /// Pauses execution for the given duration.
    pub async fn sleep(duration: Duration) {
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
