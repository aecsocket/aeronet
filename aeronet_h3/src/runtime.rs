use bevy::prelude::*;
use tokio::runtime::Runtime;

#[derive(Debug, Resource)]
pub struct AsyncRuntime(pub Runtime);

impl FromWorld for AsyncRuntime {
    fn from_world(_: &mut World) -> Self {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("failed to create tokio runtime");
        AsyncRuntime(rt)
    }
}
