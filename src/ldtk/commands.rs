use bevy::prelude::*;

use crate::ldtk::core::{LdtkCommand, LdtkCommandQueue, LdtkEntityRegistry, LdtkSpawnWorldEvent};

pub trait LdtkCommandExt {
    fn spawn_ldtk_world(&mut self, world_path: impl Into<String>);
    fn change_ldtk_level(&mut self, level_identifier: impl Into<String>);
    fn reload_ldtk_world(&mut self);
    fn unload_ldtk_world(&mut self);

    fn change_level(&mut self, level_identifier: impl Into<String>) {
        self.change_ldtk_level(level_identifier);
    }
}

impl<'w, 's> LdtkCommandExt for Commands<'w, 's> {
    fn spawn_ldtk_world(&mut self, world_path: impl Into<String>) {
        let world_path = world_path.into();
        self.queue(move |world: &mut World| {
            world
                .resource_mut::<LdtkCommandQueue>()
                .pending
                .push(LdtkCommand::SpawnWorld {
                    world_path: world_path.clone(),
                });
            world.write_message(LdtkSpawnWorldEvent { world_path });
        });
    }

    fn change_ldtk_level(&mut self, level_identifier: impl Into<String>) {
        let level_identifier = level_identifier.into();
        self.queue(move |world: &mut World| {
            world
                .resource_mut::<LdtkCommandQueue>()
                .pending
                .push(LdtkCommand::ChangeLevel { level_identifier });
        });
    }

    fn reload_ldtk_world(&mut self) {
        self.queue(move |world: &mut World| {
            world
                .resource_mut::<LdtkCommandQueue>()
                .pending
                .push(LdtkCommand::ReloadWorld);
        });
    }

    fn unload_ldtk_world(&mut self) {
        self.queue(move |world: &mut World| {
            world
                .resource_mut::<LdtkCommandQueue>()
                .pending
                .push(LdtkCommand::UnloadWorld);
        });
    }
}

pub trait LdtkAppExt {
    fn register_ldtk_entity<B>(&mut self, identifier: impl Into<String>) -> &mut Self
    where
        B: Bundle + Default + Send + Sync + 'static;

    fn register_ldtk_entity_for_layer<B>(
        &mut self,
        layer_identifier: impl Into<String>,
        entity_identifier: impl Into<String>,
    ) -> &mut Self
    where
        B: Bundle + Default + Send + Sync + 'static;

    fn register_ldtk_entity_spawner(
        &mut self,
        identifier: impl Into<String>,
        spawner: impl Fn(&mut World, Entity, &crate::ldtk::core::LdtkEntitySpawnContext)
        + Send
        + Sync
        + 'static,
    ) -> &mut Self;

    fn register_ldtk_entity_spawner_for_layer(
        &mut self,
        layer_identifier: impl Into<String>,
        entity_identifier: impl Into<String>,
        spawner: impl Fn(&mut World, Entity, &crate::ldtk::core::LdtkEntitySpawnContext)
        + Send
        + Sync
        + 'static,
    ) -> &mut Self;
}

impl LdtkAppExt for App {
    fn register_ldtk_entity<B>(&mut self, identifier: impl Into<String>) -> &mut Self
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.world_mut()
            .resource_mut::<LdtkEntityRegistry>()
            .register_bundle::<B>(identifier);
        self
    }

    fn register_ldtk_entity_for_layer<B>(
        &mut self,
        layer_identifier: impl Into<String>,
        entity_identifier: impl Into<String>,
    ) -> &mut Self
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.world_mut()
            .resource_mut::<LdtkEntityRegistry>()
            .register_bundle_for_layer::<B>(layer_identifier, entity_identifier);
        self
    }

    fn register_ldtk_entity_spawner(
        &mut self,
        identifier: impl Into<String>,
        spawner: impl Fn(&mut World, Entity, &crate::ldtk::core::LdtkEntitySpawnContext)
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.world_mut()
            .resource_mut::<LdtkEntityRegistry>()
            .register_spawner(identifier, spawner);
        self
    }

    fn register_ldtk_entity_spawner_for_layer(
        &mut self,
        layer_identifier: impl Into<String>,
        entity_identifier: impl Into<String>,
        spawner: impl Fn(&mut World, Entity, &crate::ldtk::core::LdtkEntitySpawnContext)
        + Send
        + Sync
        + 'static,
    ) -> &mut Self {
        self.world_mut()
            .resource_mut::<LdtkEntityRegistry>()
            .register_spawner_for_layer(layer_identifier, entity_identifier, spawner);
        self
    }
}
