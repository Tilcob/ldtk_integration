use bevy::prelude::*;
use bevy_ecs_ldtk::prelude::*;
use bevy_ecs_ldtk::ldtk::{LayerInstance, Level, WorldLayout, LdtkJson};
use std::path::Path;

use crate::ldtk::core::*;

pub struct GameLdtkPlugin {
    pub config: LdtkConfig,
}

impl Default for GameLdtkPlugin {
    fn default() -> Self {
        Self { config: LdtkConfig::default() }
    }
}

impl Plugin for GameLdtkPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.config.clone())
            .insert_resource(LevelSelection::default())
            .insert_resource(LdtkSettings {
                level_spawn_behavior: LevelSpawnBehavior::UseWorldTranslation {
                    load_level_neighbors: true,
                },
                ..Default::default()
            })
            .init_resource::<LdtkRuntimeState>()
            .init_resource::<LdtkMapCatalog>()
            .init_resource::<LdtkCollisionCatalog>()
            .init_resource::<LdtkRuleDatabase>()
            .init_resource::<LdtkRuleExtractionReport>()
            .init_resource::<LdtkEntityCatalog>()
            .init_resource::<LdtkGeneratedMapRequests>()
            .init_resource::<LdtkCommandQueue>()
            .init_resource::<LdtkEntityRegistry>()
            .add_message::<LdtkSpawnWorldEvent>()
            .add_message::<LdtkChangeLevelEvent>()
            .add_message::<LdtkGenerateWfcLevelEvent>()
            .add_message::<LdtkPortalTransitionEvent>()
            .add_message::<LdtkMapLoadedEvent>()
            .add_message::<LdtkMapUnloadedEvent>()
            .add_message::<LdtkLevelActivatedEvent>()
            .add_plugins((
                bevy_ecs_ldtk::LdtkPlugin,
                ImportPlugin,
                RenderingPlugin,
                CollisionPlugin,
                EntityPlugin,
                StreamingPlugin,
                WfcPlugin,
                MapManagementPlugin,
            ));
    }
}

#[derive(Default)]
pub struct ImportPlugin;

impl Plugin for ImportPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (process_import_requests, sync_level_lifecycle_events, refresh_map_catalog_from_project, debug_log_map_catalog_on_load));
    }
}

#[derive(Default)]
pub struct RenderingPlugin;

impl Plugin for RenderingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_render_layers);
    }
}

#[derive(Default)]
pub struct CollisionPlugin;

impl Plugin for CollisionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (sync_collision_layers, capture_collision_data));
    }
}

#[derive(Default)]
pub struct EntityPlugin;

impl Plugin for EntityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (capture_entity_instances, register_marked_entities, apply_registered_entity_behaviors));
    }
}

#[derive(Default)]
pub struct StreamingPlugin;

impl Plugin for StreamingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, process_streaming_requests);
    }
}

#[derive(Default)]
pub struct WfcPlugin;

impl Plugin for WfcPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (process_wfc_requests, extract_wfc_rules_from_catalog));
    }
}

#[derive(Default)]
pub struct MapManagementPlugin;

impl Plugin for MapManagementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, process_portal_transitions);
    }
}

fn process_import_requests(
    mut commands: Commands<'_, '_>,
    asset_server: Res<'_, AssetServer>,
    mut queue: ResMut<'_, LdtkCommandQueue>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
    mut selection: ResMut<'_, LevelSelection>,
    mut catalog: ResMut<'_, LdtkMapCatalog>,
    mut map_messages: MessageWriter<'_, LdtkMapLoadedEvent>,
) {
    let mut remaining = Vec::new();

    for command in queue.pending.drain(..) {
        match command {
            LdtkCommand::SpawnWorld { world_path } => {
                if let Some(root) = runtime.active_world_root.take() {
                    commands.entity(root).despawn();
                }

                let world_identifier = world_identifier_from_path(&world_path);
                let ldtk_handle = asset_server.load(world_path.clone());
                let root = commands
                    .spawn((
                        LdtkWorldBundle {
                            ldtk_handle: ldtk_handle.into(),
                            level_set: LevelSet::default(),
                            transform: Transform::default(),
                            global_transform: GlobalTransform::default(),
                            visibility: Visibility::default(),
                            inherited_visibility: InheritedVisibility::default(),
                            view_visibility: ViewVisibility::default(),
                        },
                        LdtkWorldRoot,
                        Name::new(format!("LDtk World: {world_identifier}")),
                    ))
                    .id();

                runtime.active_world = Some(world_path.clone());
                runtime.active_world_root = Some(root);
                runtime.transition = LdtkTransitionState::Loading;
                runtime.loaded_levels.clear();

                catalog.worlds.insert(
                    world_identifier.clone(),
                    LdtkWorldInfo {
                        identifier: world_identifier.clone(),
                        path: world_path,
                        levels: Vec::new(),
                        layout: LdtkWorldLayout::Free,
                    },
                );

                *selection = LevelSelection::default();
                map_messages.write(LdtkMapLoadedEvent { world_identifier });
                runtime.transition = LdtkTransitionState::Active;
            }
            other => remaining.push(other),
        }
    }

    queue.pending = remaining;
}

fn process_streaming_requests(
    mut queue: ResMut<'_, LdtkCommandQueue>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
    mut selection: ResMut<'_, LevelSelection>,
    mut level_messages: MessageWriter<'_, LdtkLevelActivatedEvent>,
) {
    let mut remaining = Vec::new();

    for command in queue.pending.drain(..) {
        match command {
            LdtkCommand::ChangeLevel { level_identifier } => {
                runtime.active_level = Some(level_identifier.clone());
                runtime.transition = LdtkTransitionState::Requested;
                *selection = LevelSelection::Identifier(level_identifier.clone());
                level_messages.write(LdtkLevelActivatedEvent { level_identifier });
                runtime.transition = LdtkTransitionState::Active;
            }
            other => remaining.push(other),
        }
    }

    queue.pending = remaining;
}

fn process_wfc_requests(
    mut queue: ResMut<'_, LdtkCommandQueue>,
    mut requests: ResMut<'_, LdtkGeneratedMapRequests>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
) {
    let mut remaining = Vec::new();

    for command in queue.pending.drain(..) {
        match command {
            LdtkCommand::GenerateWfcLevel { seed, biome } => {
                requests.pending.push(LdtkGeneratedMapRequest {
                    seed,
                    biome: biome.clone(),
                    parent_world: runtime.active_world.clone(),
                    target_level: runtime.active_level.clone(),
                });
                runtime.seed = Some(seed);
                runtime.active_biome = biome;
            }
            other => remaining.push(other),
        }
    }

    queue.pending = remaining;
}

fn process_portal_transitions(
    mut queue: ResMut<'_, LdtkCommandQueue>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
    mut selection: ResMut<'_, LevelSelection>,
    mut portal_messages: MessageWriter<'_, LdtkPortalTransitionEvent>,
    mut unload_messages: MessageWriter<'_, LdtkMapUnloadedEvent>,
) {
    let mut remaining = Vec::new();

    for command in queue.pending.drain(..) {
        match command {
            LdtkCommand::RequestPortalTransition {
                source_level,
                target_level,
                portal_id,
            } => {
                runtime.transition = LdtkTransitionState::Unloading;
                unload_messages.write(LdtkMapUnloadedEvent {
                    world_identifier: source_level.clone(),
                });
                portal_messages.write(LdtkPortalTransitionEvent {
                    source_level,
                    target_level: target_level.clone(),
                    portal_id,
                });
                runtime.active_level = Some(target_level.clone());
                *selection = LevelSelection::Identifier(target_level);
                runtime.transition = LdtkTransitionState::Active;
            }
            other => remaining.push(other),
        }
    }

    queue.pending = remaining;
}

fn sync_level_lifecycle_events(
    mut events: MessageReader<'_, '_, LevelEvent>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
    mut catalog: ResMut<'_, LdtkMapCatalog>,
) {
    for event in events.read() {
        match event {
            LevelEvent::Spawned(level_iid) => {
                let level_id = level_iid.as_str().to_string();
                runtime.loaded_levels.insert(level_id.clone());

                let world_identifier = runtime.active_world.clone().unwrap_or_default();
                catalog.levels.entry(level_id.clone()).or_insert_with(|| LdtkLevelInfo {
                    identifier: level_id,
                    world_identifier,
                    path: String::new(),
                    width: 0,
                    height: 0,
                    neighbors: Vec::new(),
                    spawn_points: Vec::new(),
                    tiles: Vec::new(),
                });
            }
            LevelEvent::Despawned(level_iid) => {
                runtime.loaded_levels.remove(level_iid.as_str());
            }
            LevelEvent::SpawnTriggered(_) | LevelEvent::Transformed(_) => {}
        }
    }
}

fn sync_render_layers(query: Query<'_, '_, (&LdtkRenderLayer, Option<&Transform>)>) {
    for (_layer, _transform) in query.iter() {
        // Render integration hook.
    }
}

fn sync_collision_layers(query: Query<'_, '_, &LdtkCollider>) {
    for _collider in query.iter() {
        // Physics integration hook.
    }
}

fn register_marked_entities(
    mut commands: Commands<'_, '_>,
    query: Query<'_, '_, (Entity, &LdtkEntityMarker), Added<LdtkEntityMarker>>,
) {
    for (entity, _marker) in query.iter() {
        commands.entity(entity).insert(LdtkPersistent);
    }
}

fn capture_entity_instances(
    mut commands: Commands<'_, '_>,
    mut catalog: ResMut<'_, LdtkEntityCatalog>,
    query: Query<'_, '_, (Entity, &EntityInstance), Added<EntityInstance>>,
) {
    for (entity, instance) in query.iter() {
        let snapshot = LdtkImportedEntity {
            entity_iid: instance.iid.clone(),
            entity_identifier: instance.identifier.clone(),
            world_identifier: None,
            level_identifier: None,
            layer_identifier: None,
            position: Vec2::new(instance.px.x as f32, instance.px.y as f32),
            grid_position: instance.grid,
            size: Vec2::new(instance.width as f32, instance.height as f32),
            pivot: instance.pivot,
            tags: instance.tags.clone(),
            field_values: instance
                .field_instances
                .iter()
                .map(|field| (field.identifier.clone(), LdtkFieldValue::from(field)))
                .collect(),
        };

        catalog.by_iid.insert(snapshot.entity_iid.clone(), entity);
        catalog.snapshots.insert(snapshot.entity_iid.clone(), snapshot.clone());

        commands.entity(entity).insert((
            snapshot,
            LdtkEntityMarker {
                definition_identifier: instance.identifier.clone(),
                level_identifier: None,
                world_identifier: None,
            },
        ));
    }
}

fn world_identifier_from_path(world_path: &str) -> String {
    Path::new(world_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(world_path)
        .to_string()
}

fn debug_log_map_catalog_on_load(
    mut events: MessageReader<'_, '_, LdtkMapLoadedEvent>,
    catalog: Res<'_, LdtkMapCatalog>,
) {
    for _ in events.read() {
        // Log concise catalog summary for interactive testing
        info!("LDtk MapCatalog: {} worlds, {} levels", catalog.worlds.len(), catalog.levels.len());
        for (iid, level) in &catalog.levels {
            info!("Level {} has {} neighbors", iid, level.neighbors.len());
            for neigh in &level.neighbors {
                info!(" - neighbor: {} dir={:?} cost={}", neigh.level_identifier, neigh.direction, neigh.cost);
            }
        }
    }
}

fn refresh_map_catalog_from_project(
    project_assets: Res<'_, Assets<LdtkProject>>,
    world_query: Query<'_, '_, &LdtkProjectHandle, With<LdtkWorldRoot>>,
    mut catalog: ResMut<'_, LdtkMapCatalog>,
) {
    let Some(handle) = world_query.iter().next() else {
        return;
    };
    let Some(project) = project_assets.get(&handle.handle) else {
        return;
    };

    let json = project.json_data();
    catalog.worlds.clear();
    catalog.levels.clear();

    if json.worlds.is_empty() {
        let world_identifier = String::from("ldtk_world");
        let levels = json.levels.iter().map(|level| level.iid.clone()).collect::<Vec<_>>();

        catalog.worlds.insert(
            world_identifier.clone(),
            LdtkWorldInfo {
                identifier: world_identifier.clone(),
                path: String::new(),
                levels,
                layout: LdtkWorldLayout::Free,
            },
        );

        for level in &json.levels {
            catalog.levels.insert(level.iid.clone(), level_info_from_level(level, &world_identifier, LdtkWorldLayout::Free));
        }
    } else {
        for world in &json.worlds {
            let layout = world.world_layout.map(world_layout_to_catalog).unwrap_or(LdtkWorldLayout::Free);
            catalog.worlds.insert(
                world.identifier.clone(),
                LdtkWorldInfo {
                    identifier: world.identifier.clone(),
                    path: String::new(),
                    levels: world.levels.iter().map(|level| level.iid.clone()).collect(),
                    layout: layout.clone(),
                },
            );

            for level in &world.levels {
                catalog.levels.insert(level.iid.clone(), level_info_from_level(level, &world.identifier, layout.clone()));
            }
        }
    }

    // Compute neighbours once all levels are in the catalog
    compute_level_neighbors_from_json(json, &mut catalog);
}

fn level_info_from_level(level: &Level, world_identifier: &str, layout: LdtkWorldLayout) -> LdtkLevelInfo {
    let mut info = LdtkLevelInfo {
        identifier: level.iid.clone(),
        world_identifier: world_identifier.to_string(),
        path: level.external_rel_path.clone().unwrap_or_default(),
        width: level.px_wid,
        height: level.px_hei,
        neighbors: Vec::new(),
        spawn_points: Vec::new(),
        tiles: Vec::new(),
    };

    if let Some(layer_instances) = &level.layer_instances {
        for layer in layer_instances {
            info.spawn_points.extend(extract_spawn_points(level, layer, world_identifier));
            info.tiles.extend(extract_tile_metadata(layer));
        }
    }

    info.neighbors = match layout {
        LdtkWorldLayout::LinearHorizontal | LdtkWorldLayout::LinearVertical => Vec::new(),
        _ => Vec::new(),
    };

    info
}

// Build a neighbour graph for all levels using LDtk's neighbour hints and world positions.
fn compute_level_neighbors_from_json(json: &LdtkJson, catalog: &mut LdtkMapCatalog) {
    use std::collections::HashMap as Map;

    // Build lookup IID -> Level
    let mut iid_map: Map<String, &Level> = Map::new();
    if json.worlds.is_empty() {
        for level in &json.levels {
            iid_map.insert(level.iid.clone(), level);
        }
    } else {
        for world in &json.worlds {
            for level in &world.levels {
                iid_map.insert(level.iid.clone(), level);
            }
        }
    }

    // Helper: infer cardinal direction from world coordinates when available
    let direction_by_pos = |a: &Level, b: &Level| -> Option<LdtkDirection> {
        if a.world_x == -1 || a.world_y == -1 || b.world_x == -1 || b.world_y == -1 {
            return None;
        }
        let dx = b.world_x - a.world_x;
        let dy = b.world_y - a.world_y;
        if dx.abs() > dy.abs() {
            if dx > 0 { Some(LdtkDirection::East) } else { Some(LdtkDirection::West) }
        } else {
            if dy < 0 { Some(LdtkDirection::North) } else { Some(LdtkDirection::South) }
        }
    };

    // Recompute neighbors for each catalog level
    for (iid, info) in catalog.levels.iter_mut() {
        info.neighbors.clear();

        if let Some(level) = iid_map.get(iid) {
            for neigh in &level.neighbours {
                let target_iid = neigh.level_iid.clone();
                let dir_hint = neigh.dir.as_str();

                match dir_hint {
                    "n" | "N" | "north" => info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::North, cost: 1.0 }),
                    "s" | "S" | "south" => info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::South, cost: 1.0 }),
                    "e" | "E" | "east" => info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::East, cost: 1.0 }),
                    "w" | "W" | "west" => info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::West, cost: 1.0 }),
                    "o" => {
                        let dir = iid_map.get(&target_iid).and_then(|t| direction_by_pos(level, t)).unwrap_or(LdtkDirection::North);
                        info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: dir, cost: 0.2 });
                    }
                    "<" | ">" => {
                        let dir = iid_map.get(&target_iid).and_then(|t| direction_by_pos(level, t)).unwrap_or(LdtkDirection::North);
                        info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: dir, cost: 1.5 });
                    }
                    diag if diag.len() == 2 && (diag.contains('n') || diag.contains('s') || diag.contains('e') || diag.contains('w')) => {
                        if diag.contains('n') { info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::North, cost: 1.4 }); }
                        if diag.contains('s') { info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::South, cost: 1.4 }); }
                        if diag.contains('e') { info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::East, cost: 1.4 }); }
                        if diag.contains('w') { info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::West, cost: 1.4 }); }
                    }
                    _ => {
                        if let Some(target_level) = iid_map.get(&target_iid) {
                            if let Some(dir) = direction_by_pos(level, target_level) {
                                info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: dir, cost: 1.0 });
                            } else {
                                info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::North, cost: 1.0 });
                            }
                        } else {
                            info.neighbors.push(LdtkNeighbor { level_identifier: target_iid.clone(), direction: LdtkDirection::North, cost: 1.0 });
                        }
                    }
                }
            }
        }
    }
}

fn extract_spawn_points(level: &Level, layer: &LayerInstance, world_identifier: &str) -> Vec<LdtkSpawnPoint> {
    layer
        .entity_instances
        .iter()
        .filter(|entity| {
            let identifier = entity.identifier.to_lowercase();
            identifier.contains("spawn") || entity.tags.iter().any(|tag| tag.eq_ignore_ascii_case("spawn"))
        })
        .map(|entity| LdtkSpawnPoint {
            identifier: entity.identifier.clone(),
            position: Vec2::new(entity.px.x as f32, entity.px.y as f32),
            level_identifier: level.iid.clone(),
            tags: {
                let mut tags = entity.tags.clone();
                if tags.is_empty() {
                    tags.push(world_identifier.to_string());
                }
                tags
            },
        })
        .collect()
}

fn extract_tile_metadata(layer: &LayerInstance) -> Vec<LdtkTileMetadata> {
    layer
        .grid_tiles
        .iter()
        .chain(layer.auto_layer_tiles.iter())
        .map(|tile| LdtkTileMetadata {
            tileset_identifier: layer
                .tileset_rel_path
                .clone()
                .unwrap_or_else(|| layer.identifier.clone()),
            tile_id: tile.t,
            source_position: tile.src,
            rotation_degrees: tile_rotation_from_flags(tile.f),
            flip_x: tile.f & 1 != 0,
            flip_y: tile.f & 2 != 0,
            weight: tile.a.max(0.0),
            tags: Vec::new(),
        })
        .collect()
}

fn tile_rotation_from_flags(flags: i32) -> u16 {
    match flags & 3 {
        0 => 0,
        1 => 0,
        2 => 180,
        3 => 180,
        _ => 0,
    }
}

fn world_layout_to_catalog(layout: WorldLayout) -> LdtkWorldLayout {
    match layout {
        WorldLayout::Free => LdtkWorldLayout::Free,
        WorldLayout::GridVania => LdtkWorldLayout::GridVania,
        WorldLayout::LinearHorizontal => LdtkWorldLayout::LinearHorizontal,
        WorldLayout::LinearVertical => LdtkWorldLayout::LinearVertical,
    }
}

fn capture_collision_data(
    mut commands: Commands<'_, '_>,
    mut catalog: ResMut<'_, LdtkCollisionCatalog>,
    int_grid_query: Query<'_, '_, (Entity, &IntGridCell, Option<&GridCoords>, Option<&LayerMetadata>), Added<IntGridCell>>,
    layer_query: Query<'_, '_, &LayerMetadata, Added<LayerMetadata>>,
) {
    for layer in layer_query.iter() {
        let key = format!("{}:{}", layer.level_id, layer.identifier);
        catalog.layers.entry(key).or_insert_with(|| LdtkCollisionLayerInfo {
            level_identifier: layer.level_id.to_string(),
            layer_identifier: layer.identifier.clone(),
            layer_type: format!("{:?}", layer.layer_instance_type),
            solid_cells: 0,
            tile_cells: 0,
        });
    }

    for (entity, cell, grid, layer) in int_grid_query.iter() {
        let layer_identifier = layer.map(|metadata| metadata.identifier.clone()).unwrap_or_else(|| String::from("IntGrid"));
        let level_identifier = layer.map(|metadata| metadata.level_id.to_string()).unwrap_or_default();
        let grid_position = grid.map(|coords| IVec2::new(coords.x, coords.y)).unwrap_or(IVec2::ZERO);
        let solid = cell.value != 0;

        catalog.cells.push(LdtkCollisionCell {
            level_identifier: level_identifier.clone(),
            layer_identifier: layer_identifier.clone(),
            grid_position,
            solid,
            source: String::from("IntGrid"),
        });

        if solid {
            commands.entity(entity).insert(LdtkCollider { solid: true, sensor: false });
        }

        let entry = catalog.layers.entry(format!("{level_identifier}:{layer_identifier}")).or_insert_with(|| LdtkCollisionLayerInfo {
            level_identifier,
            layer_identifier,
            layer_type: String::from("IntGrid"),
            solid_cells: 0,
            tile_cells: 0,
        });
        entry.solid_cells += usize::from(solid);
    }
}

fn apply_registered_entity_behaviors(
    mut commands: Commands<'_, '_>,
    registry: Res<'_, LdtkEntityRegistry>,
    query: Query<'_, '_, (Entity, &LdtkImportedEntity), Added<LdtkImportedEntity>>,
) {
    for (entity, imported) in query.iter() {
        if registry
            .resolve(imported.layer_identifier.as_deref(), &imported.entity_identifier)
            .is_some()
        {
            let context = LdtkEntitySpawnContext {
                entity_iid: imported.entity_iid.clone(),
                entity_identifier: imported.entity_identifier.clone(),
                world_identifier: imported.world_identifier.clone(),
                level_identifier: imported.level_identifier.clone(),
                layer_identifier: imported.layer_identifier.clone(),
                position: imported.position,
                grid_position: imported.grid_position,
                size: imported.size,
                pivot: imported.pivot,
                tags: imported.tags.clone(),
                tile: None,
                field_values: imported.field_values.clone(),
            };
            let layer_identifier = imported.layer_identifier.clone();
            let entity_identifier = imported.entity_identifier.clone();

            commands.queue(move |world: &mut World| {
                world.resource_scope(|world, registry: Mut<'_, LdtkEntityRegistry>| {
                    if let Some(spawner) = registry.resolve(layer_identifier.as_deref(), &entity_identifier) {
                        spawner(world, entity, &context);
                    }
                });
            });
        }
    }
}

fn extract_wfc_rules_from_catalog(
    catalog: Res<'_, LdtkMapCatalog>,
    mut rule_db: ResMut<'_, LdtkRuleDatabase>,
    mut report: ResMut<'_, LdtkRuleExtractionReport>,
) {
    rule_db.signatures.clear();
    rule_db.compatibility.clear();
    rule_db.weights.clear();
    report.levels_scanned = catalog.levels.len();
    report.tiles_scanned = 0;
    report.observations_created = 0;

    for level in catalog.levels.values() {
        for tile in &level.tiles {
            let signature = LdtkTileSignature {
                tileset_identifier: tile.tileset_identifier.clone(),
                tile_id: tile.tile_id,
                rotation_degrees: tile.rotation_degrees,
                flip_x: tile.flip_x,
                flip_y: tile.flip_y,
            };
            let key = tile_key(&signature);
            rule_db.signatures.insert(key.clone(), signature);
            rule_db.weights.entry(key).and_modify(|weight| *weight += tile.weight).or_insert(tile.weight);
            report.tiles_scanned += 1;
        }

        for neighbor in &level.neighbors {
            rule_db.compatibility.entry(level.identifier.clone()).or_default().push(neighbor.level_identifier.clone());
            report.observations_created += 1;
        }
    }
}

fn tile_key(signature: &LdtkTileSignature) -> String {
    format!(
        "{}:{}:{}:{}:{}",
        signature.tileset_identifier,
        signature.tile_id,
        signature.rotation_degrees,
        signature.flip_x,
        signature.flip_y
    )
}















