use bevy_core::Name;
use bevy_ecs::prelude::*;

pub(crate) fn display_name(entity: Entity, name: Option<&Name>) -> String {
    if let Some(name) = name {
        format!("'{name}' ({entity:?})")
    } else {
        format!("{entity:?}")
    }
}
