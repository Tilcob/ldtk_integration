//! Stealth-puzzle example: doors connect levels via LDtk entity fields.
//!
//! # Required LDtk project setup
//!
//! Create an entity definition `Door` in your LDtk project with two custom
//! fields:
//!
//! | Field name      | Type   | Example value  |
//! |-----------------|--------|----------------|
//! | `target_level`  | String | `"Dungeon_02"` |
//! | `target_spawn`  | String | `"Entrance_A"` |
//!
//! Also place a `PlayerSpawn` entity (or give any entity the tag `spawn`) in
//! every level — the level manager picks it up automatically.
//!
//! # Running
//!
//! ```
//! cargo run --example stealth_doors
//! ```
//!
//! The example expects `assets/worlds/stealth.ldtk`. Without it the app starts
//! but no levels load and all events remain silent.

use bevy::prelude::*;
use ldtk_integration::{
    GameLdtkPlugin, LdtkAppExt, LdtkCollisionReadyEvent, LdtkCommandExt, LdtkConfig,
    LdtkEntitySpawnContext, LdtkFieldAccess, LdtkLevelManagerConfig, LdtkLevelPlayer,
    LdtkLevelReadyEvent, LdtkLoadState, LdtkMapLoadedEvent, LevelManagerPlugin,
    LevelTransitionState, LevelTransitionStatus, LdtkCollisionRule,
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        // ── Core LDtk plugin ───────────────────────────────────────────────────
        .add_plugins(GameLdtkPlugin::new(
            LdtkConfig::default()
                .with_world_asset_path("worlds/stealth.ldtk")
                // IntGrid value 1 = wall (solid), 2 = vision zone (sensor)
                .with_collision_rules([
                    LdtkCollisionRule::solid(1).for_layer("Collision"),
                    LdtkCollisionRule::sensor(2, "vision_zone").for_layer("Gameplay"),
                ]),
        ))
        // ── Level manager ──────────────────────────────────────────────────────
        .add_plugins(LevelManagerPlugin)
        .insert_resource(LdtkLevelManagerConfig {
            default_spawn_tag: "PlayerSpawn".to_string(),
            default_spawn_identifier: "PlayerSpawn".to_string(),
            allow_missing_spawnpoints: false,
            ..Default::default()
        })
        // ── Entity registration ────────────────────────────────────────────────
        .register_ldtk_entity_spawner("Door", spawn_door)
        // ── Systems ────────────────────────────────────────────────────────────
        .add_systems(Startup, (setup_camera, spawn_player_entity))
        .add_systems(
            Update,
            (
                on_map_loaded,
                on_level_ready,
                on_collision_ready,
                handle_door_interaction,
                log_transition_failures,
            ),
        )
        .run();
}

// ── Components ─────────────────────────────────────────────────────────────────

/// A door that transitions the player to another level when entered.
/// Fields are read from the LDtk project at spawn time.
#[derive(Component)]
struct Door {
    target_level: String,
    target_spawn: String,
}

/// Marks the player entity. [`LevelManagerPlugin`] teleports this entity to
/// the resolved spawn point after every level transition.
#[derive(Component)]
struct Player;

// ── Startup systems ────────────────────────────────────────────────────────────

fn setup_camera(mut commands: Commands) {
    commands.spawn((Camera2d, Name::new("Camera")));
}

fn spawn_player_entity(mut commands: Commands) {
    commands.spawn((
        Player,
        LdtkLevelPlayer, // required for automatic teleport by LevelManagerPlugin
        Transform::default(),
        GlobalTransform::default(),
        Name::new("Player"),
    ));
}

// ── Entity spawner ─────────────────────────────────────────────────────────────

/// Called by the plugin when `bevy_ecs_ldtk` spawns a `Door` entity from the
/// LDtk project. Reads custom fields and attaches the [`Door`] component.
fn spawn_door(world: &mut World, entity: Entity, ctx: &LdtkEntitySpawnContext) {
    let target_level = ctx.field_str("target_level").unwrap_or("").to_string();
    let target_spawn = ctx.field_str("target_spawn").unwrap_or("").to_string();

    if target_level.is_empty() {
        warn!(
            "Door '{}' has no `target_level` field set.",
            ctx.entity_identifier
        );
    }

    world.entity_mut(entity).insert((
        Door {
            target_level: target_level.clone(),
            target_spawn,
        },
        Name::new(format!("Door → {target_level}")),
    ));
}

// ── Update systems ─────────────────────────────────────────────────────────────

/// Reacts to the LDtk world finishing its initial load and logs statistics.
fn on_map_loaded(
    mut events: MessageReader<LdtkMapLoadedEvent>,
    load_state: Res<LdtkLoadState>,
) {
    for event in events.read() {
        info!(
            "LDtk world '{}' loaded — {} level(s), {} layers, {} tilesets",
            event.world_identifier,
            load_state.stats.levels,
            load_state.stats.layers,
            load_state.stats.tilesets,
        );

        for warning in &load_state.warnings {
            warn!("LDtk validation: {}", warning);
        }
    }
}

/// Reacts to a completed level transition (player has been teleported).
fn on_level_ready(mut events: MessageReader<LdtkLevelReadyEvent>) {
    for event in events.read() {
        info!(
            "Level '{}' ready — player at ({:.0}, {:.0})",
            event.level_identifier, event.position.x, event.position.y,
        );
    }
}

/// Reacts when collision data for a level has been fully captured.
/// Wire a physics adapter (e.g. Rapier or Avian) here to build colliders
/// from [`ldtk_integration::LdtkCollisionCatalog`].
fn on_collision_ready(mut events: MessageReader<LdtkCollisionReadyEvent>) {
    for event in events.read() {
        info!(
            "Collision for '{}' ready — {} cells",
            event.level_identifier, event.cells,
        );
        // Example: read LdtkCollisionCatalog here and create Rapier colliders.
    }
}

/// Triggers a level transition when the player walks close to a door.
///
/// In a real game replace the distance check with a physics sensor event
/// (e.g. `EventReader<CollisionEvent>` from Rapier or Avian). The
/// [`LdtkCommandExt::transition_to_ldtk_level`] call is the same either way.
fn handle_door_interaction(
    mut commands: Commands,
    player_query: Query<&Transform, With<Player>>,
    door_query: Query<(&Door, &Transform)>,
    transition_state: Res<LevelTransitionState>,
) {
    // Do not start a new transition while one is already in progress.
    if transition_state.status == LevelTransitionStatus::WaitingForSpawn {
        return;
    }

    let Ok(player_tf) = player_query.single() else {
        return;
    };

    for (door, door_tf) in door_query.iter() {
        let distance = player_tf
            .translation
            .truncate()
            .distance(door_tf.translation.truncate());

        if distance < 24.0 {
            info!(
                "Entering door → level '{}', spawn '{}'",
                door.target_level, door.target_spawn,
            );
            commands.transition_to_ldtk_level(
                door.target_level.clone(),
                Some(door.target_spawn.clone()),
            );
            return; // handle at most one door per frame
        }
    }
}

/// Logs an error whenever a level transition fails (unknown level identifier
/// or missing spawn point).
fn log_transition_failures(state: Res<LevelTransitionState>) {
    if state.is_changed() {
        if let (LevelTransitionStatus::Failed, Some(error)) = (&state.status, &state.error) {
            error!("Level transition failed: {}", error);
        }
    }
}
