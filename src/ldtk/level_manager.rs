use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_ecs_ldtk::prelude::{LevelEvent, LevelSelection};
#[cfg(feature = "tilemap")]
use bevy_ecs_tilemap::prelude::{TilePos, TileTextureIndex, TilemapId};
#[cfg(feature = "tilemap")]
use std::collections::HashMap;
use std::time::Duration;

use crate::ldtk::core::{
    LdtkCollisionCatalog, LdtkConfig, LdtkEntityMarker, LdtkLoadSet, LdtkLoadState, LdtkLoadStatus,
    LdtkMapCatalog, LdtkPersistent, LdtkRuntimeState, LdtkSpawnPoint, LdtkTileAnimator,
    LdtkValidationReport,
};
#[cfg(feature = "tilemap")]
use crate::ldtk::core::{LdtkLayerInfo, LdtkLevelInfo, LdtkTileAnimation};

pub struct LevelManagerPlugin;

impl Plugin for LevelManagerPlugin {
    fn build(&self, app: &mut App) {
        // The loader-side resources (LdtkRuntimeState, LdtkMapCatalog, LdtkConfig,
        // ...) are owned by GameLdtkPlugin. LevelManagerPlugin only adds its own
        // transition state and guards its systems on those resources existing, so
        // that adding it without GameLdtkPlugin neither panics nor silently does
        // nothing — it logs a clear error at startup (see check_loader_dependency).
        app.init_resource::<CurrentLdtkLevel>()
            .init_resource::<PendingLdtkLevelTransition>()
            .init_resource::<LevelTransitionState>()
            .init_resource::<LdtkLevelManagerConfig>()
            .init_resource::<LdtkPlayerLocator>()
            .add_message::<LevelTransitionRequest>()
            .add_message::<LdtkLevelReadyEvent>()
            .add_message::<LdtkCollisionReadyEvent>()
            .add_systems(Startup, check_loader_dependency)
            .add_systems(
                Update,
                (
                    request_initial_level_transition,
                    handle_transition_requests,
                    finalize_level_transition,
                )
                    .chain()
                    .in_set(LdtkLoadSet::LevelTransitions)
                    .run_if(resource_exists::<LdtkRuntimeState>),
            );

        #[cfg(feature = "tilemap")]
        {
            app.init_resource::<LdtkTileAnimationLookup>().add_systems(
                Update,
                (
                    rebuild_tile_animation_lookup,
                    attach_tile_animators_to_tiles,
                    apply_tile_animation_to_tilemap,
                )
                    .chain()
                    .in_set(LdtkLoadSet::Animation)
                    .run_if(resource_exists::<LdtkMapCatalog>),
            );
        }
    }
}

/// Emits a clear error at startup if `LevelManagerPlugin` was added without
/// `GameLdtkPlugin`. `LdtkRuntimeState` is initialized exclusively by the loader
/// plugin, so its absence is an unambiguous signal that the dependency is missing
/// — in which case the transition systems are skipped (see `run_if` above) rather
/// than panicking on a missing resource.
fn check_loader_dependency(runtime: Option<Res<'_, LdtkRuntimeState>>) {
    if runtime.is_none() {
        error!(
            "LevelManagerPlugin requires GameLdtkPlugin, which provides LDtk loading and the \
             LdtkMapCatalog. Without it, level transitions are disabled. Add GameLdtkPlugin to \
             your App before LevelManagerPlugin."
        );
    }
}

#[derive(Debug, Clone, Resource)]
pub struct LdtkLevelManagerConfig {
    pub default_spawn_tag: String,
    pub default_spawn_identifier: String,
    pub enable_tile_animation_adapter: bool,
    pub allow_missing_spawnpoints: bool,
}

impl Default for LdtkLevelManagerConfig {
    fn default() -> Self {
        Self {
            default_spawn_tag: "PlayerSpawn".to_string(),
            default_spawn_identifier: "PlayerSpawn".to_string(),
            enable_tile_animation_adapter: false,
            allow_missing_spawnpoints: false,
        }
    }
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkPlayerLocator {
    pub entity: Option<Entity>,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct CurrentLdtkLevel {
    pub identifier: Option<String>,
    pub iid: Option<String>,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct PendingLdtkLevelTransition {
    pub target_level: Option<String>,
    pub spawn_id: Option<String>,
    pub target_level_iid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum LevelTransitionStatus {
    #[default]
    Idle,
    WaitingForSpawn,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LevelTransitionState {
    pub status: LevelTransitionStatus,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Message)]
pub struct LevelTransitionRequest {
    pub target_level: String,
    pub spawn_id: Option<String>,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkLevelReadyEvent {
    pub level_identifier: String,
    pub spawn_id: Option<String>,
    pub position: Vec2,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkCollisionReadyEvent {
    pub level_identifier: String,
    pub cells: usize,
}

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkLevelPlayer;

#[derive(Debug, Clone, Component)]
pub struct LdtkLevelScoped {
    pub level_identifier: String,
}

#[cfg(feature = "tilemap")]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LdtkTileAnimationKey {
    level_iid: String,
    layer_iid: String,
    grid_pos: IVec2,
}

#[cfg(feature = "tilemap")]
#[derive(Debug, Clone, Resource, Default)]
struct LdtkTileAnimationLookup {
    by_tile: HashMap<LdtkTileAnimationKey, LdtkTileAnimation>,
}

#[derive(SystemParam)]
struct TransitionResources<'w> {
    pending: ResMut<'w, PendingLdtkLevelTransition>,
    state: ResMut<'w, LevelTransitionState>,
    current: ResMut<'w, CurrentLdtkLevel>,
    runtime: ResMut<'w, LdtkRuntimeState>,
    catalog: Res<'w, LdtkMapCatalog>,
    collision_catalog: Res<'w, LdtkCollisionCatalog>,
    config: Res<'w, LdtkLevelManagerConfig>,
    strict_config: Res<'w, LdtkConfig>,
    load_state: ResMut<'w, LdtkLoadState>,
    validation: ResMut<'w, LdtkValidationReport>,
}

/// Auto-promotes the first level that `bevy_ecs_ldtk` spawns into a full level
/// transition when nothing else has driven one yet (Bug 3). Without this, a
/// world loaded via `LdtkConfig::with_world_asset_path(..)` — or a bare
/// `change_ldtk_level(..)` — would render but never place the player, set
/// `CurrentLdtkLevel`, or fire `LdtkLevelReadyEvent` / `LdtkCollisionReadyEvent`,
/// so a single-level game would start with nothing wired up.
///
/// Runs only while idle (no current level and no pending transition) so it never
/// competes with an explicit `transition_to_ldtk_level` request. The emitted
/// request is picked up by `handle_transition_requests` in the same frame, and
/// `finalize_level_transition` completes it against the same `Spawned` event.
fn request_initial_level_transition(
    mut events: MessageReader<'_, '_, LevelEvent>,
    current: Res<'_, CurrentLdtkLevel>,
    pending: Res<'_, PendingLdtkLevelTransition>,
    catalog: Res<'_, LdtkMapCatalog>,
    mut requests: MessageWriter<'_, LevelTransitionRequest>,
) {
    let idle = current.identifier.is_none() && pending.target_level.is_none();
    let mut requested = false;
    for event in events.read() {
        // Drain every event so the reader cursor stays current even when idle is
        // false; only the first resolvable spawn triggers a request.
        let LevelEvent::Spawned(level_iid) = event else {
            continue;
        };
        if !idle || requested {
            continue;
        }
        if let Some(identifier) = catalog.identifier_for_iid(level_iid.as_str()) {
            requests.write(LevelTransitionRequest {
                target_level: identifier.to_string(),
                spawn_id: None,
            });
            requested = true;
        }
    }
}

fn handle_transition_requests(
    mut requests: MessageReader<'_, '_, LevelTransitionRequest>,
    mut selection: ResMut<'_, LevelSelection>,
    mut pending: ResMut<'_, PendingLdtkLevelTransition>,
    mut state: ResMut<'_, LevelTransitionState>,
    catalog: Res<'_, LdtkMapCatalog>,
    config: Res<'_, LdtkConfig>,
    mut load_state: ResMut<'_, LdtkLoadState>,
    mut validation: ResMut<'_, LdtkValidationReport>,
) {
    for request in requests.read() {
        if let Err(error) = start_transition(
            &mut pending,
            &mut state,
            request.target_level.clone(),
            request.spawn_id.clone(),
            &catalog,
        ) {
            let strict = config.strict_validation;
            state.status = LevelTransitionStatus::Failed;
            state.error = Some(error.clone());
            validation.push(strict, "transition_level_missing", error);
            if strict {
                load_state.status = LdtkLoadStatus::Error;
            }
            continue;
        }
        *selection = LevelSelection::Identifier(request.target_level.clone());
    }
}

fn start_transition(
    pending: &mut PendingLdtkLevelTransition,
    state: &mut LevelTransitionState,
    target_level: String,
    spawn_id: Option<String>,
    catalog: &LdtkMapCatalog,
) -> Result<(), String> {
    let target_level_iid = catalog
        .level_by_id_or_iid(&target_level)
        .map(|info| info.iid.clone())
        .ok_or_else(|| format!("Level '{target_level}' not found in LdtkMapCatalog"))?;

    pending.target_level = Some(target_level);
    pending.spawn_id = spawn_id;
    pending.target_level_iid = Some(target_level_iid);
    state.status = LevelTransitionStatus::WaitingForSpawn;
    state.error = None;
    Ok(())
}

fn finalize_level_transition(
    mut commands: Commands<'_, '_>,
    mut events: MessageReader<'_, '_, LevelEvent>,
    mut ready_messages: MessageWriter<'_, LdtkLevelReadyEvent>,
    mut collision_messages: MessageWriter<'_, LdtkCollisionReadyEvent>,
    mut resources: TransitionResources<'_>,
    locator: Res<'_, LdtkPlayerLocator>,
    player_query: Query<'_, '_, Entity, With<LdtkLevelPlayer>>,
    mut transform_query: Query<'_, '_, &mut Transform>,
    cleanup_query: Query<
        '_,
        '_,
        (Entity, Option<&LdtkEntityMarker>, Option<&LdtkLevelScoped>),
        (Without<LdtkPersistent>, Without<LdtkLevelPlayer>),
    >,
) {
    let Some(target_level) = resources.pending.target_level.clone() else {
        return;
    };

    for event in events.read() {
        let LevelEvent::Spawned(level_iid) = event else {
            continue;
        };
        let level_iid = level_iid.as_str().to_string();
        let level_identifier = level_identifier_from_iid(&resources.catalog, &level_iid);

        let matches_pending = resources
            .pending
            .target_level_iid
            .as_ref()
            .is_some_and(|iid| iid == &level_iid)
            || level_identifier
                .as_ref()
                .is_some_and(|identifier| identifier == &target_level);
        if !matches_pending {
            continue;
        }

        let level_identifier = level_identifier.unwrap_or_else(|| target_level.clone());
        let spawn = match resolve_spawn_point(
            &resources.catalog,
            &level_identifier,
            resources.pending.spawn_id.as_deref(),
            &resources.config,
        ) {
            Ok(spawn) => spawn,
            Err(message) => {
                let strict = resources.strict_config.strict_validation;
                resources.state.status = LevelTransitionStatus::Failed;
                resources.state.error = Some(message.clone());
                resources
                    .validation
                    .push(strict, "transition_spawn_missing", message);
                if strict {
                    resources.load_state.status = LdtkLoadStatus::Error;
                }
                resources.pending.target_level = None;
                resources.pending.target_level_iid = None;
                resources.pending.spawn_id = None;
                return;
            }
        };

        if resources.current.identifier.as_deref() != Some(&level_identifier) {
            if let Some(old_identifier) = resources.current.identifier.as_deref() {
                cleanup_level_entities(&mut commands, &cleanup_query, old_identifier);
            }
        }

        // Capture the spawn id before clearing `pending` (Bug 4): the ready event
        // below still needs it, but every `pending` field must be reset so a
        // second `Spawned` event in the same frame (neighbor streaming) cannot
        // re-match a stale `target_level_iid`/`spawn_id` and teleport twice.
        let spawn_id = resources.pending.spawn_id.clone();
        resources.current.identifier = Some(level_identifier.clone());
        resources.current.iid = Some(level_iid.clone());
        resources.runtime.active_level = Some(level_identifier.clone());
        resources.state.status = LevelTransitionStatus::Ready;
        resources.state.error = None;
        resources.pending.target_level = None;
        resources.pending.target_level_iid = None;
        resources.pending.spawn_id = None;

        // Teleport the player to the resolved spawn point. Prefer the explicit
        // locator entity, fall back to the first `LdtkLevelPlayer`, and warn
        // loudly if neither resolves to a live Transform (Bug 5) instead of
        // silently leaving the player in place.
        let player_entity = locator
            .entity
            .filter(|&entity| transform_query.contains(entity))
            .or_else(|| {
                player_query
                    .iter()
                    .find(|&entity| transform_query.contains(entity))
            });
        match player_entity.and_then(|entity| transform_query.get_mut(entity).ok()) {
            Some(mut transform) => {
                transform.translation =
                    Vec3::new(spawn.position.x, spawn.position.y, transform.translation.z);
            }
            None => {
                warn!(
                    "No LdtkLevelPlayer/locator entity with a Transform found — \
                     skipping teleport for level '{level_identifier}'."
                );
            }
        }

        ready_messages.write(LdtkLevelReadyEvent {
            level_identifier: level_identifier.clone(),
            spawn_id,
            position: spawn.position,
        });

        let collision_cells = resources
            .collision_catalog
            .cells
            .iter()
            .filter(|cell| cell.level_identifier == level_identifier)
            .count();
        collision_messages.write(LdtkCollisionReadyEvent {
            level_identifier,
            cells: collision_cells,
        });

        // One transition completes per finalize pass; stop so additional
        // `Spawned` events this frame are not mistaken for this transition.
        break;
    }
}

fn cleanup_level_entities(
    commands: &mut Commands<'_, '_>,
    query: &Query<
        '_,
        '_,
        (Entity, Option<&LdtkEntityMarker>, Option<&LdtkLevelScoped>),
        (Without<LdtkPersistent>, Without<LdtkLevelPlayer>),
    >,
    level_identifier: &str,
) {
    for (entity, marker, scoped) in query.iter() {
        let marker_level = marker.and_then(|marker| marker.level_identifier.as_deref());
        let scoped_level = scoped.map(|scope| scope.level_identifier.as_str());
        // The query already excludes persistent entities and the player, so
        // those flags are known-false here; the shared predicate still encodes
        // the full rule and is exercised directly by the unit tests.
        if should_cleanup_entity(marker_level, scoped_level, false, false, level_identifier) {
            commands.entity(entity).despawn();
        }
    }
}

fn should_cleanup_entity(
    marker_level: Option<&str>,
    scoped_level: Option<&str>,
    is_persistent: bool,
    is_player: bool,
    target_level: &str,
) -> bool {
    if is_persistent || is_player {
        return false;
    }
    marker_level == Some(target_level) || scoped_level == Some(target_level)
}

fn resolve_spawn_point(
    catalog: &LdtkMapCatalog,
    target_level: &str,
    spawn_id: Option<&str>,
    config: &LdtkLevelManagerConfig,
) -> Result<LdtkSpawnPoint, String> {
    let level = catalog
        .levels
        .get(target_level)
        .ok_or_else(|| format!("Level '{target_level}' not found in LdtkMapCatalog"))?;

    if let Some(spawn_id) = spawn_id {
        // Identifier and tag matching are both case-insensitive so that
        // `transition_to_ldtk_level("L", Some("playerspawn"))` resolves
        // `PlayerSpawn` regardless of casing (Bug 6).
        let found = level.spawn_points.iter().find(|spawn| {
            spawn.identifier.eq_ignore_ascii_case(spawn_id)
                || spawn
                    .tags
                    .iter()
                    .any(|tag| tag.eq_ignore_ascii_case(spawn_id))
        });
        return found
            .cloned()
            .ok_or_else(|| format!("Spawnpoint '{spawn_id}' not found in level '{target_level}'"));
    }

    let default_spawn = level.spawn_points.iter().find(|spawn| {
        spawn
            .identifier
            .eq_ignore_ascii_case(&config.default_spawn_identifier)
            || spawn
                .tags
                .iter()
                .any(|tag| tag.eq_ignore_ascii_case(&config.default_spawn_tag))
    });
    if let Some(spawn) = default_spawn {
        return Ok(spawn.clone());
    }

    if let Some(spawn) = level.spawn_points.first() {
        return Ok(spawn.clone());
    }

    if config.allow_missing_spawnpoints {
        return Ok(LdtkSpawnPoint {
            identifier: String::from("Fallback"),
            position: Vec2::ZERO,
            level_identifier: target_level.to_string(),
            layer_identifier: String::from(""),
            tags: Vec::new(),
        });
    }

    Err(format!("Level '{target_level}' has no spawnpoints"))
}

fn level_identifier_from_iid(catalog: &LdtkMapCatalog, iid: &str) -> Option<String> {
    catalog.identifier_for_iid(iid).map(ToOwned::to_owned)
}

#[cfg(feature = "tilemap")]
fn rebuild_tile_animation_lookup(
    catalog: Res<'_, LdtkMapCatalog>,
    config: Res<'_, LdtkLevelManagerConfig>,
    mut lookup: ResMut<'_, LdtkTileAnimationLookup>,
) {
    if !catalog.is_changed() || !config.enable_tile_animation_adapter {
        return;
    }

    lookup.by_tile = build_tile_animation_lookup(&catalog.levels, &catalog.layers);
}

#[cfg(feature = "tilemap")]
fn build_tile_animation_lookup(
    levels: &HashMap<String, LdtkLevelInfo>,
    layers: &HashMap<String, LdtkLayerInfo>,
) -> HashMap<LdtkTileAnimationKey, LdtkTileAnimation> {
    let mut lookup = HashMap::new();

    for level in levels.values() {
        for tile in &level.tiles {
            let Some(animation) = tile.animation.clone() else {
                continue;
            };
            let Some(layer) = layers.get(&tile.layer_iid) else {
                continue;
            };
            if layer.grid_size <= 0 {
                continue;
            }

            let grid_pos = IVec2::new(
                tile.layer_position.x / layer.grid_size,
                tile.layer_position.y / layer.grid_size,
            );

            let key = LdtkTileAnimationKey {
                level_iid: level.iid.clone(),
                layer_iid: tile.layer_iid.clone(),
                grid_pos,
            };
            lookup.insert(key, animation.clone());
        }
    }

    lookup
}

#[cfg(feature = "tilemap")]
fn attach_tile_animators_to_tiles(
    mut commands: Commands<'_, '_>,
    lookup: Res<'_, LdtkTileAnimationLookup>,
    config: Res<'_, LdtkLevelManagerConfig>,
    catalog: Res<'_, LdtkMapCatalog>,
    mut tile_query: Query<
        '_,
        '_,
        (Entity, &TilePos, &TilemapId, &mut TileTextureIndex),
        Without<LdtkTileAnimator>,
    >,
    layer_query: Query<'_, '_, &bevy_ecs_ldtk::prelude::LayerMetadata>,
) {
    if lookup.by_tile.is_empty() || !config.enable_tile_animation_adapter {
        return;
    }

    for (entity, pos, tilemap_id, mut texture_index) in tile_query.iter_mut() {
        let Ok(layer_meta) = layer_query.get(tilemap_id.0) else {
            continue;
        };
        let level_iid = resolve_level_iid_from_metadata(&catalog, &layer_meta.level_id.to_string());
        let key = LdtkTileAnimationKey {
            level_iid,
            layer_iid: layer_meta.iid.clone(),
            grid_pos: IVec2::new(pos.x as i32, pos.y as i32),
        };
        let Some(animation) = lookup.by_tile.get(&key) else {
            continue;
        };
        if let Some(first) = animation.frames.first() {
            texture_index.0 = first.tile_id as u32;
        }
        commands
            .entity(entity)
            .insert(LdtkTileAnimator::new(animation.clone()));
    }
}

#[cfg(feature = "tilemap")]
fn resolve_level_iid_from_metadata(catalog: &LdtkMapCatalog, level_id: &str) -> String {
    // `level_id` may already be an iid, or an identifier we need to translate.
    catalog
        .level_by_id_or_iid(level_id)
        .map(|level| level.iid.clone())
        .unwrap_or_else(|| level_id.to_string())
}

#[cfg(feature = "tilemap")]
fn apply_tile_animation_to_tilemap(
    mut query: Query<'_, '_, (&LdtkTileAnimator, &mut TileTextureIndex)>,
) {
    for (animator, mut texture_index) in query.iter_mut() {
        let Some(frame) = animator.animation.frames.get(animator.frame_index) else {
            continue;
        };
        texture_index.0 = frame.tile_id as u32;
    }
}

pub fn advance_tile_animation(animator: &mut LdtkTileAnimator, delta: Duration) -> Option<i32> {
    animator.advance(delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ldtk::core::{LdtkLevelInfo, LdtkTileAnimation};

    fn build_catalog_with_spawnpoints() -> LdtkMapCatalog {
        let mut catalog = LdtkMapCatalog::default();
        let mut level = LdtkLevelInfo::default();
        level.identifier = "Level_A".to_string();
        level.spawn_points = vec![
            LdtkSpawnPoint {
                identifier: "PlayerSpawn".to_string(),
                position: Vec2::new(10.0, 20.0),
                tags: vec!["PlayerSpawn".to_string()],
                level_identifier: "Level_A".to_string(),
                layer_identifier: "Entities".to_string(),
            },
            LdtkSpawnPoint {
                identifier: "Alt".to_string(),
                position: Vec2::new(30.0, 40.0),
                tags: vec!["Alt".to_string()],
                level_identifier: "Level_A".to_string(),
                layer_identifier: "Entities".to_string(),
            },
        ];
        catalog.insert_level_info(level);
        catalog
    }

    #[test]
    fn resolves_explicit_spawn_id() {
        let catalog = build_catalog_with_spawnpoints();
        let config = LdtkLevelManagerConfig::default();

        let spawn =
            resolve_spawn_point(&catalog, "Level_A", Some("Alt"), &config).expect("spawnpoint");

        assert_eq!(spawn.identifier, "Alt");
    }

    #[test]
    fn resolves_spawn_id_case_insensitively() {
        let catalog = build_catalog_with_spawnpoints();
        let config = LdtkLevelManagerConfig::default();

        // Lower-case query must resolve the `PlayerSpawn` identifier (Bug 6).
        let spawn = resolve_spawn_point(&catalog, "Level_A", Some("playerspawn"), &config)
            .expect("spawnpoint");
        assert_eq!(spawn.identifier, "PlayerSpawn");

        // And the alternate spawn by its identifier, also case-insensitively.
        let alt =
            resolve_spawn_point(&catalog, "Level_A", Some("ALT"), &config).expect("spawnpoint");
        assert_eq!(alt.identifier, "Alt");
    }

    #[test]
    fn falls_back_to_default_spawnpoint() {
        let catalog = build_catalog_with_spawnpoints();
        let config = LdtkLevelManagerConfig::default();

        let spawn = resolve_spawn_point(&catalog, "Level_A", None, &config).expect("spawnpoint");

        assert_eq!(spawn.identifier, "PlayerSpawn");
    }

    #[test]
    fn missing_spawnpoint_returns_error() {
        let mut catalog = LdtkMapCatalog::default();
        let mut level = LdtkLevelInfo::default();
        level.identifier = "Level_A".to_string();
        catalog.insert_level_info(level);

        let result = resolve_spawn_point(
            &catalog,
            "Level_A",
            Some("Missing"),
            &LdtkLevelManagerConfig::default(),
        );

        assert!(result.is_err());
    }

    #[test]
    fn advances_tile_animation_state() {
        let animation = LdtkTileAnimation {
            frames: vec![
                crate::ldtk::core::LdtkTileAnimationFrame {
                    tile_id: 1,
                    duration: 0.05,
                },
                crate::ldtk::core::LdtkTileAnimationFrame {
                    tile_id: 2,
                    duration: 0.05,
                },
            ],
            repeat: true,
        };
        let mut animator = LdtkTileAnimator::new(animation);

        let frame = advance_tile_animation(&mut animator, Duration::from_millis(60));

        assert_eq!(frame, Some(2));
    }

    #[test]
    fn transition_state_changes_on_request() {
        let catalog = build_catalog_with_spawnpoints();
        let mut pending = PendingLdtkLevelTransition::default();
        let mut state = LevelTransitionState::default();

        let result = start_transition(
            &mut pending,
            &mut state,
            "Level_A".to_string(),
            None,
            &catalog,
        );

        assert!(result.is_ok());
        assert_eq!(state.status, LevelTransitionStatus::WaitingForSpawn);
        assert_eq!(pending.target_level.as_deref(), Some("Level_A"));
    }

    #[test]
    fn transition_state_fails_for_unknown_level() {
        let catalog = build_catalog_with_spawnpoints();
        let mut pending = PendingLdtkLevelTransition::default();
        let mut state = LevelTransitionState::default();

        let result = start_transition(
            &mut pending,
            &mut state,
            "Missing".to_string(),
            None,
            &catalog,
        );

        assert!(result.is_err());
    }

    #[test]
    fn cleanup_decision_respects_persistence() {
        assert!(!should_cleanup_entity(
            Some("Level_A"),
            None,
            true,
            false,
            "Level_A"
        ));
        assert!(!should_cleanup_entity(
            Some("Level_A"),
            None,
            false,
            true,
            "Level_A"
        ));
        assert!(should_cleanup_entity(
            Some("Level_A"),
            None,
            false,
            false,
            "Level_A"
        ));
        assert!(!should_cleanup_entity(
            Some("Level_B"),
            None,
            false,
            false,
            "Level_A"
        ));
    }

    #[test]
    fn allows_fallback_spawnpoint_when_enabled() {
        let mut catalog = LdtkMapCatalog::default();
        let mut level = LdtkLevelInfo::default();
        level.identifier = "Level_A".to_string();
        catalog.insert_level_info(level);

        let mut config = LdtkLevelManagerConfig::default();
        config.allow_missing_spawnpoints = true;
        let spawn = resolve_spawn_point(&catalog, "Level_A", None, &config).expect("fallback");

        assert_eq!(spawn.identifier, "Fallback");
        assert_eq!(spawn.position, Vec2::ZERO);
    }
}
