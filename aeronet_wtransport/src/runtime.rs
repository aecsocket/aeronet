use bevy::prelude::*;

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
