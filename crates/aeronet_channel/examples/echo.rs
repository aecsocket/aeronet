use bevy::prelude::*;

fn main() -> AppExit {
    App::new().add_plugins((DefaultPlugins, EguiPlugin)).run()
}
