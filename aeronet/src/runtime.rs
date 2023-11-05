use bevy::prelude::*;

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
/// use aeronet::AsyncRuntime;
/// use bevy::prelude::*;
///
/// App::new().init_resource::<AsyncRuntime>();
/// ```
///
/// Then add the [`AsyncRuntime`] as a [`Res`] system parameter:
///
/// ```
/// # use bevy::prelude::*;
/// # use aeronet::AsyncRuntime;
/// fn run_something_async(rt: Res<AsyncRuntime>) {
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
pub struct AsyncRuntime(pub tokio::runtime::Runtime);

impl FromWorld for AsyncRuntime {
    fn from_world(_: &mut World) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("should be able to create async runtime");
        Self(rt)
    }
}
