use bevy_ecs::{prelude::*, system::EntityCommands};

pub trait InitComponentExt {
    fn init_component<C: Component + FromWorld>(&mut self) -> &mut Self;
}

impl InitComponentExt for EntityCommands<'_> {
    fn init_component<C: Component + FromWorld>(&mut self) -> &mut Self {
        self.add(|entity: Entity, world: &mut World| {
            if world.entity(entity).contains::<C>() {
                return;
            }

            let component = <C as FromWorld>::from_world(world);
            world.entity_mut(entity).insert(component);
        })
    }
}
