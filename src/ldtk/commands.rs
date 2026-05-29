use bevy::prelude::*;

use crate::ldtk::core::{
    LdtkChangeLevelEvent, LdtkCommand, LdtkCommandQueue, LdtkEntityRegistry,
    LdtkGenerateWfcLevelEvent, LdtkPortalTransitionEvent, LdtkSpawnWorldEvent,
};

pub trait LdtkCommandExt {
    fn spawn_ldtk_world(&mut self, world_path: impl Into<String>);
    fn change_level(&mut self, level_identifier: impl Into<String>);
    fn generate_wfc_level(&mut self, seed: u64);
    fn generate_wfc_level_with_biome(&mut self, seed: u64, biome: impl Into<String>);
    fn request_portal_transition(
        &mut self,
        source_level: impl Into<String>,
        target_level: impl Into<String>,
        portal_id: impl Into<String>,
    );
}

impl<'w, 's> LdtkCommandExt for Commands<'w, 's> {
    fn spawn_ldtk_world(&mut self, world_path: impl Into<String>) {
        let world_path = world_path.into();
        self.queue(move |world: &mut World| {
            world.resource_mut::<LdtkCommandQueue>().pending.push(LdtkCommand::SpawnWorld {
                world_path: world_path.clone(),
            });
            world.write_message(LdtkSpawnWorldEvent { world_path });
        });
    }

    fn change_level(&mut self, level_identifier: impl Into<String>) {
        let level_identifier = level_identifier.into();
        self.queue(move |world: &mut World| {
            world.resource_mut::<LdtkCommandQueue>().pending.push(LdtkCommand::ChangeLevel {
                level_identifier: level_identifier.clone(),
            });
            world.write_message(LdtkChangeLevelEvent { level_identifier });
        });
    }

    fn generate_wfc_level(&mut self, seed: u64) {
        self.queue(move |world: &mut World| {
            world.resource_mut::<LdtkCommandQueue>().pending.push(LdtkCommand::GenerateWfcLevel {
                seed,
                biome: None,
            });
            world.write_message(LdtkGenerateWfcLevelEvent { seed, biome: None });
        });
    }

    fn generate_wfc_level_with_biome(&mut self, seed: u64, biome: impl Into<String>) {
        let biome = biome.into();
        self.queue(move |world: &mut World| {
            world.resource_mut::<LdtkCommandQueue>().pending.push(LdtkCommand::GenerateWfcLevel {
                seed,
                biome: Some(biome.clone()),
            });
            world.write_message(LdtkGenerateWfcLevelEvent {
                seed,
                biome: Some(biome),
            });
        });
    }

    fn request_portal_transition(
        &mut self,
        source_level: impl Into<String>,
        target_level: impl Into<String>,
        portal_id: impl Into<String>,
    ) {
        let source_level = source_level.into();
        let target_level = target_level.into();
        let portal_id = portal_id.into();

        self.queue(move |world: &mut World| {
            world.resource_mut::<LdtkCommandQueue>().pending.push(LdtkCommand::RequestPortalTransition {
                source_level: source_level.clone(),
                target_level: target_level.clone(),
                portal_id: portal_id.clone(),
            });
            world.write_message(LdtkPortalTransitionEvent {
                source_level,
                target_level,
                portal_id,
            });
        });
    }
}

pub trait LdtkAppExt {
    fn register_ldtk_entity<B>(&mut self, identifier: impl Into<String>)
    where
        B: Bundle + Default + Send + Sync + 'static;
}

impl LdtkAppExt for App {
    fn register_ldtk_entity<B>(&mut self, identifier: impl Into<String>)
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        let mut registry = self.world_mut().resource_mut::<LdtkEntityRegistry>();
        registry.register_bundle::<B>(identifier);
    }
}




