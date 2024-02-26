use std::ops::{Deref, DerefMut};

use bevy_ecs::prelude::*;

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
/// use bevy_app::prelude::*;
///
/// App::new().init_resource::<TokioRuntime>();
/// ```
///
/// Then add the [`TokioRuntime`] as a [`Res`] system parameter:
///
/// ```
/// # use bevy_ecs::prelude::*;
/// # use aeronet::TokioRuntime;
/// fn run_something_async(rt: Res<TokioRuntime>) {
///     rt.spawn(async move {
///         do_the_async_thing();
///     });
/// }
///
/// async fn do_the_async_thing() {}
/// ```
///
/// If the runtime cannot be created when initialized, the app will panic.
///
/// [`App::init_resource`]: bevy_app::prelude::App::init_resource
#[derive(Debug, Resource)]
pub struct TokioRuntime(pub tokio::runtime::Runtime);

impl Default for TokioRuntime {
    fn default() -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("should be able to create tokio runtime");
        Self(rt)
    }
}

impl Deref for TokioRuntime {
    type Target = tokio::runtime::Runtime;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for TokioRuntime {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[cfg(test)]
mod tests {
    use bevy_app::prelude::*;

    use super::*;

    #[test]
    fn resource_in_app() {
        let mut app = App::new();
        app.init_resource::<TokioRuntime>()
            .add_systems(Update, use_rt);

        app.update();
    }

    fn use_rt(_: Res<TokioRuntime>) {}
}
