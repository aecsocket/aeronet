use bevy::prelude::*;
use tokio::runtime;

/// Wrapper resource around an async [`tokio`] runtime.
///
/// Some transports may require an async runtime for handling connections, and
/// Bevy does not provide one by default. This provides a
/// [`tokio::runtime::Runtime`] wrapped in a [`Resource`] which can be injected
/// into any system.
///
/// # Usage
///
/// To insert into a [`World`], initialize the resource using
/// [`App::init_resource`]:
///
/// ```
/// use aeronet::TokioRuntime;
/// use bevy::prelude::*;
///
/// App::new().init_resource::<TokioRuntime>();
/// ```
///
/// Then add the [`TokioRuntime`] as a [`Res`] system parameter:
///
/// ```
/// # use bevy::prelude::*;
/// # use aeronet::TokioRuntime;
/// fn run_something_async(rt: Res<TokioRuntime>) {
///     rt.0.spawn(async move {
///         do_the_async_thing();
///     });
/// }
///
/// async fn do_the_async_thing() {}
/// ```
///
/// If the runtime cannot be created when initialized, the app will panic.
#[derive(Debug, Resource)]
pub struct TokioRuntime(pub runtime::Runtime);

impl Default for TokioRuntime {
    fn default() -> Self {
        let rt = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("should be able to create tokio runtime");
        Self(rt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_in_app() {
        let mut app = App::new();
        app.init_resource::<TokioRuntime>();

        app.update();
    }
}
