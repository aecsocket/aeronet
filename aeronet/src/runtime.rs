use bevy::prelude::*;
use tokio::runtime;

#[derive(Debug, Resource)]
pub struct AsyncRuntime(pub runtime::Runtime);

impl Default for AsyncRuntime {
    fn default() -> Self {
        let rt = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("should be able to create async runtime");
        Self(rt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn async_runtime_resource() {
        let mut app = App::new();
        app.init_resource::<AsyncRuntime>();

        app.update();
    }
}
