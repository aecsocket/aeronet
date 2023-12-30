use bevy::prelude::*;
use tokio::runtime;

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
