use bevy::prelude::*;
use bevy_ecs_ldtk::ldtk::{
    LayerInstance, LdtkJson, Level, TileInstance, TilesetDefinition, TilesetRectangle, WorldLayout,
};
use bevy_ecs_ldtk::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::ldtk::core::*;

pub struct GameLdtkPlugin {
    pub config: LdtkConfig,
}

impl Default for GameLdtkPlugin {
    fn default() -> Self {
        Self {
            config: LdtkConfig::default(),
        }
    }
}

impl GameLdtkPlugin {
    pub fn new(config: LdtkConfig) -> Self {
        Self { config }
    }
}

impl Plugin for GameLdtkPlugin {
    fn build(&self, app: &mut App) {
        let level_spawn_behavior = LevelSpawnBehavior::UseWorldTranslation {
            load_level_neighbors: self.config.load_level_neighbors,
        };

        app.insert_resource(self.config.clone())
            .insert_resource(LevelSelection::default())
            .insert_resource(LdtkSettings {
                level_spawn_behavior,
                ..Default::default()
            })
            .init_resource::<LdtkRuntimeState>()
            .init_resource::<LdtkLoadState>()
            .init_resource::<LdtkValidationReport>()
            .init_resource::<LdtkMapCatalog>()
            .init_resource::<LdtkCollisionCatalog>()
            .init_resource::<LdtkEntityCatalog>()
            .init_resource::<LdtkCommandQueue>()
            .init_resource::<LdtkEntityRegistry>()
            .add_message::<LdtkSpawnWorldEvent>()
            .add_message::<LdtkMapLoadedEvent>()
            .add_message::<LdtkLevelActivatedEvent>()
            .add_message::<LdtkWorldUnloadedEvent>()
            .add_message::<LdtkValidationFinishedEvent>()
            .add_plugins(bevy_ecs_ldtk::LdtkPlugin)
            .add_systems(Startup, spawn_configured_world)
            .add_systems(
                Update,
                (
                    process_ldtk_commands,
                    refresh_map_catalog_from_project,
                    sync_level_lifecycle_events,
                    capture_collision_data,
                    capture_entity_instances,
                    register_marked_entities,
                    apply_registered_entity_behaviors,
                    tick_ldtk_tile_animators,
                ),
            );
    }
}

fn spawn_configured_world(mut commands: Commands<'_, '_>, config: Res<'_, LdtkConfig>) {
    if let Some(world_path) = &config.world_asset_path {
        queue_spawn_world(&mut commands, world_path.clone());
    }
}

fn process_ldtk_commands(
    mut commands: Commands<'_, '_>,
    asset_server: Res<'_, AssetServer>,
    mut queue: ResMut<'_, LdtkCommandQueue>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
    mut load_state: ResMut<'_, LdtkLoadState>,
    mut selection: ResMut<'_, LevelSelection>,
    mut level_messages: MessageWriter<'_, LdtkLevelActivatedEvent>,
    mut unload_messages: MessageWriter<'_, LdtkWorldUnloadedEvent>,
) {
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

                runtime.active_world_path = Some(world_path);
                runtime.active_world_identifier = Some(world_identifier);
                runtime.active_world_root = Some(root);
                runtime.active_level = None;
                runtime.loaded_levels.clear();
                runtime.transition = LdtkTransitionState::Loading;
                load_state.status = LdtkLoadStatus::Loading;
                load_state.world_identifier = runtime.active_world_identifier.clone();
                load_state.warnings.clear();
                load_state.errors.clear();
                load_state.stats = LdtkLoadStats::default();
                *selection = LevelSelection::default();
            }
            LdtkCommand::ChangeLevel { level_identifier } => {
                runtime.active_level = Some(level_identifier.clone());
                *selection = LevelSelection::Identifier(level_identifier.clone());
                level_messages.write(LdtkLevelActivatedEvent { level_identifier });
            }
            LdtkCommand::ReloadWorld => {
                if let Some(world_path) = runtime.active_world_path.clone() {
                    queue_spawn_world(&mut commands, world_path);
                }
            }
            LdtkCommand::UnloadWorld => {
                if let Some(root) = runtime.active_world_root.take() {
                    commands.entity(root).despawn();
                }
                runtime.active_world_path = None;
                runtime.active_world_identifier = None;
                runtime.active_level = None;
                runtime.loaded_levels.clear();
                runtime.transition = LdtkTransitionState::Idle;
                load_state.status = LdtkLoadStatus::NotLoaded;
                load_state.world_identifier = None;
                load_state.stats = LdtkLoadStats::default();
                *selection = LevelSelection::default();
                unload_messages.write(LdtkWorldUnloadedEvent);
            }
            LdtkCommand::None => {}
        }
    }
}

fn queue_spawn_world(commands: &mut Commands<'_, '_>, world_path: String) {
    commands.queue(move |world: &mut World| {
        world
            .resource_mut::<LdtkCommandQueue>()
            .pending
            .push(LdtkCommand::SpawnWorld {
                world_path: world_path.clone(),
            });
        world.write_message(LdtkSpawnWorldEvent { world_path });
    });
}

fn refresh_map_catalog_from_project(
    project_assets: Res<'_, Assets<LdtkProject>>,
    world_query: Query<'_, '_, &LdtkProjectHandle, With<LdtkWorldRoot>>,
    mut catalog: ResMut<'_, LdtkMapCatalog>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
    registry: Res<'_, LdtkEntityRegistry>,
    config: Res<'_, LdtkConfig>,
    mut load_state: ResMut<'_, LdtkLoadState>,
    mut validation: ResMut<'_, LdtkValidationReport>,
    mut map_messages: MessageWriter<'_, LdtkMapLoadedEvent>,
    mut validation_messages: MessageWriter<'_, LdtkValidationFinishedEvent>,
) {
    let Some(handle) = world_query.iter().next() else {
        return;
    };
    let Some(project) = project_assets.get(&handle.handle) else {
        return;
    };

    let json = project.json_data();
    let active_world_path = runtime.active_world_path.clone().unwrap_or_default();

    catalog.worlds.clear();
    catalog.levels.clear();
    catalog.layers.clear();
    catalog.tilesets = extract_tilesets(json);
    catalog.tile_animations = catalog
        .tilesets
        .values()
        .flat_map(|tileset| {
            tileset.custom_data.iter().filter_map(|(tile_id, data)| {
                parse_tile_animation(data).map(|animation| {
                    (
                        LdtkTileKey {
                            tileset_uid: Some(tileset.uid),
                            tile_id: *tile_id,
                        },
                        animation,
                    )
                })
            })
        })
        .collect();

    if json.worlds.is_empty() {
        let world_identifier = world_identifier_from_path(&active_world_path);
        let levels = json
            .levels
            .iter()
            .map(|level| level.identifier.clone())
            .collect::<Vec<_>>();

        catalog.worlds.insert(
            world_identifier.clone(),
            LdtkWorldInfo {
                identifier: world_identifier.clone(),
                path: active_world_path.clone(),
                levels,
                layout: LdtkWorldLayout::Free,
            },
        );

        for level in &json.levels {
            let level = level_with_external_data(level, &active_world_path, &config)
                .unwrap_or_else(|| level.clone());
            insert_level(
                &level,
                &world_identifier,
                LdtkWorldLayout::Free,
                &mut catalog,
                &config,
            );
        }
    } else {
        for world in &json.worlds {
            let layout = world
                .world_layout
                .map(world_layout_to_catalog)
                .unwrap_or_default();
            let level_identifiers = world
                .levels
                .iter()
                .map(|level| level.identifier.clone())
                .collect::<Vec<_>>();

            catalog.worlds.insert(
                world.identifier.clone(),
                LdtkWorldInfo {
                    identifier: world.identifier.clone(),
                    path: active_world_path.clone(),
                    levels: level_identifiers,
                    layout: layout.clone(),
                },
            );

            for level in &world.levels {
                let level = level_with_external_data(level, &active_world_path, &config)
                    .unwrap_or_else(|| level.clone());
                insert_level(
                    &level,
                    &world.identifier,
                    layout.clone(),
                    &mut catalog,
                    &config,
                );
            }
        }
    }

    compute_level_neighbors_from_json(json, &mut catalog);
    update_load_stats(&catalog, &mut load_state);

    if config.validate_on_load {
        validate_catalog(&catalog, &registry, &config, &mut validation);
        load_state.warnings = validation
            .warnings
            .iter()
            .map(|issue| format!("{}: {}", issue.code, issue.message))
            .collect();
        load_state.errors = validation
            .errors
            .iter()
            .map(|issue| format!("{}: {}", issue.code, issue.message))
            .collect();
        validation_messages.write(LdtkValidationFinishedEvent {
            warnings: validation.warnings.len(),
            errors: validation.errors.len(),
        });
    }

    if runtime.transition == LdtkTransitionState::Loading && !catalog.is_empty() {
        let world_identifier = runtime
            .active_world_identifier
            .clone()
            .unwrap_or_else(|| world_identifier_from_path(&active_world_path));
        load_state.world_identifier = Some(world_identifier.clone());
        if validation.has_errors() {
            load_state.status = LdtkLoadStatus::Error;
        } else {
            runtime.transition = LdtkTransitionState::Active;
            load_state.status = LdtkLoadStatus::Ready;
            map_messages.write(LdtkMapLoadedEvent { world_identifier });
        }
    }
}

fn insert_level(
    level: &Level,
    world_identifier: &str,
    layout: LdtkWorldLayout,
    catalog: &mut LdtkMapCatalog,
    config: &LdtkConfig,
) {
    let mut info = level_info_from_level(level, world_identifier);

    if let Some(layer_instances) = &level.layer_instances {
        for layer in layer_instances {
            if !config.should_include_layer(&layer.identifier) {
                continue;
            }

            let layer_info = layer_info_from_layer(layer, &info.identifier);
            catalog.layers.insert(layer.iid.clone(), layer_info);

            info.spawn_points
                .extend(extract_spawn_points(level, layer, world_identifier));
            info.tiles.extend(extract_tile_metadata(
                level,
                layer,
                &catalog.tilesets,
                &catalog.tile_animations,
            ));
            info.entities.extend(extract_entity_snapshots(
                level,
                layer,
                world_identifier,
                &catalog.tilesets,
                &catalog.tile_animations,
            ));
        }
    }

    if matches!(
        layout,
        LdtkWorldLayout::LinearHorizontal | LdtkWorldLayout::LinearVertical
    ) {
        info.neighbors.clear();
    }

    catalog.levels.insert(info.identifier.clone(), info);
}

fn level_with_external_data(
    level: &Level,
    active_world_path: &str,
    config: &LdtkConfig,
) -> Option<Level> {
    if !config.catalog_external_levels || level.layer_instances.is_some() {
        return None;
    }

    let external_path = level.external_rel_path.as_ref()?;
    let full_path = external_level_path(&config.asset_root, active_world_path, external_path);
    let text = fs::read_to_string(full_path).ok()?;
    serde_json::from_str::<Level>(&text).ok()
}

fn external_level_path(asset_root: &str, active_world_path: &str, external_path: &str) -> PathBuf {
    let world_dir = Path::new(active_world_path)
        .parent()
        .unwrap_or_else(|| Path::new(""));
    Path::new(asset_root).join(world_dir).join(external_path)
}

fn level_info_from_level(level: &Level, world_identifier: &str) -> LdtkLevelInfo {
    LdtkLevelInfo {
        iid: level.iid.clone(),
        identifier: level.identifier.clone(),
        world_identifier: world_identifier.to_string(),
        external_path: level.external_rel_path.clone(),
        size: IVec2::new(level.px_wid, level.px_hei),
        world_position: IVec2::new(level.world_x, level.world_y),
        neighbors: Vec::new(),
        spawn_points: Vec::new(),
        tiles: Vec::new(),
        entities: Vec::new(),
        fields: level
            .field_instances
            .iter()
            .map(|field| (field.identifier.clone(), LdtkFieldValue::from(field)))
            .collect(),
    }
}

fn layer_info_from_layer(layer: &LayerInstance, level_identifier: &str) -> LdtkLayerInfo {
    LdtkLayerInfo {
        iid: layer.iid.clone(),
        identifier: layer.identifier.clone(),
        level_identifier: level_identifier.to_string(),
        layer_type: format!("{:?}", layer.layer_instance_type),
        grid_size: layer.grid_size,
        grid_size_cells: IVec2::new(layer.c_wid, layer.c_hei),
        tileset_uid: layer.override_tileset_uid.or(layer.tileset_def_uid),
        tileset_rel_path: layer.tileset_rel_path.clone(),
        opacity: layer.opacity,
        visible: layer.visible,
    }
}

fn update_load_stats(catalog: &LdtkMapCatalog, load_state: &mut LdtkLoadState) {
    load_state.stats = LdtkLoadStats {
        worlds: catalog.worlds.len(),
        levels: catalog.levels.len(),
        layers: catalog.layers.len(),
        tilesets: catalog.tilesets.len(),
        tiles: catalog.levels.values().map(|level| level.tiles.len()).sum(),
        entities: catalog
            .levels
            .values()
            .map(|level| level.entities.len())
            .sum(),
        spawn_points: catalog
            .levels
            .values()
            .map(|level| level.spawn_points.len())
            .sum(),
        collision_cells: 0,
        tile_animations: catalog.tile_animations.len(),
    };
}

fn validate_catalog(
    catalog: &LdtkMapCatalog,
    registry: &LdtkEntityRegistry,
    config: &LdtkConfig,
    report: &mut LdtkValidationReport,
) {
    report.clear();

    for level in catalog.levels.values() {
        if level.external_path.is_some() && level.tiles.is_empty() && level.entities.is_empty() {
            report.warnings.push(LdtkValidationIssue::warning(
                "external_level_not_cataloged",
                format!(
                    "Level '{}' references an external .ldtkl file. bevy_ecs_ldtk can load it, but this metadata catalog only sees embedded layer data.",
                    level.identifier
                ),
            ));
        }

        if level.spawn_points.is_empty() {
            report.warnings.push(LdtkValidationIssue::warning(
                "missing_spawn_point",
                format!(
                    "Level '{}' has no entity tagged/named as spawn.",
                    level.identifier
                ),
            ));
        }

        if config.warn_on_unregistered_entities {
            for entity in &level.entities {
                if registry
                    .resolve(
                        entity.layer_identifier.as_deref(),
                        &entity.entity_identifier,
                    )
                    .is_none()
                {
                    report.warnings.push(LdtkValidationIssue::warning(
                        "unregistered_entity",
                        format!(
                            "LDtk entity '{}' in level '{}' has no registered bundle/spawner.",
                            entity.entity_identifier, level.identifier
                        ),
                    ));
                }
            }
        }
    }

    for layer in catalog.layers.values() {
        if layer.tileset_uid.is_some() && layer.tileset_rel_path.is_none() {
            report.warnings.push(LdtkValidationIssue::warning(
                "missing_tileset_path",
                format!(
                    "Layer '{}' in level '{}' references a tileset without a relative path.",
                    layer.identifier, layer.level_identifier
                ),
            ));
        }
    }
}

fn extract_tilesets(json: &LdtkJson) -> HashMap<i32, LdtkTilesetInfo> {
    json.defs
        .tilesets
        .iter()
        .map(|tileset| (tileset.uid, tileset_info_from_definition(tileset)))
        .collect()
}

fn tileset_info_from_definition(tileset: &TilesetDefinition) -> LdtkTilesetInfo {
    let mut tile_tags: HashMap<i32, Vec<String>> = HashMap::new();
    for tag in &tileset.enum_tags {
        for tile_id in &tag.tile_ids {
            tile_tags
                .entry(*tile_id)
                .or_default()
                .push(tag.enum_value_id.clone());
        }
    }

    LdtkTilesetInfo {
        uid: tileset.uid,
        identifier: tileset.identifier.clone(),
        rel_path: tileset.rel_path.clone(),
        tile_grid_size: tileset.tile_grid_size,
        grid_size_cells: IVec2::new(tileset.c_wid, tileset.c_hei),
        image_size: IVec2::new(tileset.px_wid, tileset.px_hei),
        spacing: tileset.spacing,
        padding: tileset.padding,
        tags: tileset.tags.clone(),
        tile_tags,
        custom_data: tileset
            .custom_data
            .iter()
            .map(|entry| (entry.tile_id, entry.data.clone()))
            .collect(),
    }
}

fn compute_level_neighbors_from_json(json: &LdtkJson, catalog: &mut LdtkMapCatalog) {
    let mut levels_by_iid: HashMap<String, &Level> = HashMap::new();
    if json.worlds.is_empty() {
        for level in &json.levels {
            levels_by_iid.insert(level.iid.clone(), level);
        }
    } else {
        for world in &json.worlds {
            for level in &world.levels {
                levels_by_iid.insert(level.iid.clone(), level);
            }
        }
    }

    let identifiers_by_iid = levels_by_iid
        .iter()
        .map(|(iid, level)| (iid.clone(), level.identifier.clone()))
        .collect::<HashMap<_, _>>();

    for info in catalog.levels.values_mut() {
        info.neighbors.clear();

        let Some(level) = levels_by_iid.get(&info.iid) else {
            continue;
        };

        for neighbor in &level.neighbours {
            let target_identifier = identifiers_by_iid
                .get(&neighbor.level_iid)
                .cloned()
                .unwrap_or_else(|| neighbor.level_iid.clone());
            let direction = parse_neighbor_direction(&neighbor.dir)
                .or_else(|| {
                    levels_by_iid
                        .get(&neighbor.level_iid)
                        .and_then(|target| direction_by_world_position(level, target))
                })
                .unwrap_or_default();
            let cost = neighbor_cost(&neighbor.dir);

            info.neighbors.push(LdtkNeighbor {
                level_identifier: target_identifier,
                direction,
                cost,
            });
        }
    }
}

fn parse_neighbor_direction(value: &str) -> Option<LdtkDirection> {
    match value {
        "n" | "N" | "north" => Some(LdtkDirection::North),
        "s" | "S" | "south" => Some(LdtkDirection::South),
        "e" | "E" | "east" => Some(LdtkDirection::East),
        "w" | "W" | "west" => Some(LdtkDirection::West),
        _ => None,
    }
}

fn neighbor_cost(value: &str) -> f32 {
    match value {
        "o" => 0.2,
        "<" | ">" => 1.5,
        diagonal if diagonal.len() == 2 => 1.4,
        _ => 1.0,
    }
}

fn direction_by_world_position(source: &Level, target: &Level) -> Option<LdtkDirection> {
    if source.world_x == -1 || source.world_y == -1 || target.world_x == -1 || target.world_y == -1
    {
        return None;
    }

    let delta = IVec2::new(
        target.world_x - source.world_x,
        target.world_y - source.world_y,
    );
    if delta.x.abs() > delta.y.abs() {
        Some(if delta.x > 0 {
            LdtkDirection::East
        } else {
            LdtkDirection::West
        })
    } else {
        Some(if delta.y < 0 {
            LdtkDirection::North
        } else {
            LdtkDirection::South
        })
    }
}

fn extract_spawn_points(
    level: &Level,
    layer: &LayerInstance,
    world_identifier: &str,
) -> Vec<LdtkSpawnPoint> {
    layer
        .entity_instances
        .iter()
        .filter(|entity| {
            let identifier = entity.identifier.to_lowercase();
            identifier.contains("spawn")
                || entity
                    .tags
                    .iter()
                    .any(|tag| tag.eq_ignore_ascii_case("spawn"))
        })
        .map(|entity| {
            let mut tags = entity.tags.clone();
            if tags.is_empty() {
                tags.push(world_identifier.to_string());
            }

            LdtkSpawnPoint {
                identifier: entity.identifier.clone(),
                position: Vec2::new(entity.px.x as f32, entity.px.y as f32),
                level_identifier: level.identifier.clone(),
                layer_identifier: layer.identifier.clone(),
                tags,
            }
        })
        .collect()
}

fn extract_tile_metadata(
    level: &Level,
    layer: &LayerInstance,
    tilesets: &HashMap<i32, LdtkTilesetInfo>,
    animations: &HashMap<LdtkTileKey, LdtkTileAnimation>,
) -> Vec<LdtkTileMetadata> {
    let tileset_uid = layer.override_tileset_uid.or(layer.tileset_def_uid);
    let tileset_identifier = tileset_uid
        .and_then(|uid| tilesets.get(&uid).map(|tileset| tileset.identifier.clone()))
        .or_else(|| layer.tileset_rel_path.clone())
        .unwrap_or_else(|| layer.identifier.clone());

    layer
        .grid_tiles
        .iter()
        .chain(layer.auto_layer_tiles.iter())
        .map(|tile| {
            tile_metadata_from_instance(
                level,
                layer,
                tile,
                tileset_uid,
                &tileset_identifier,
                tilesets,
                animations,
            )
        })
        .collect()
}

fn tile_metadata_from_instance(
    level: &Level,
    layer: &LayerInstance,
    tile: &TileInstance,
    tileset_uid: Option<i32>,
    tileset_identifier: &str,
    tilesets: &HashMap<i32, LdtkTilesetInfo>,
    animations: &HashMap<LdtkTileKey, LdtkTileAnimation>,
) -> LdtkTileMetadata {
    let key = LdtkTileKey {
        tileset_uid,
        tile_id: tile.t,
    };

    LdtkTileMetadata {
        level_identifier: level.identifier.clone(),
        layer_identifier: layer.identifier.clone(),
        layer_iid: layer.iid.clone(),
        tileset_uid,
        tileset_identifier: tileset_identifier.to_string(),
        tile_id: tile.t,
        layer_position: tile.px,
        source_position: tile.src,
        rotation_degrees: tile_rotation_from_flags(tile.f),
        flip_x: tile.f & 1 != 0,
        flip_y: tile.f & 2 != 0,
        alpha: tile.a,
        custom_data: tileset_uid
            .and_then(|uid| tilesets.get(&uid))
            .and_then(|tileset| tileset.custom_data.get(&tile.t).cloned()),
        tags: tileset_uid
            .and_then(|uid| tilesets.get(&uid))
            .and_then(|tileset| tileset.tile_tags.get(&tile.t).cloned())
            .unwrap_or_default(),
        animation: animations.get(&key).cloned(),
    }
}

fn extract_entity_snapshots(
    level: &Level,
    layer: &LayerInstance,
    world_identifier: &str,
    tilesets: &HashMap<i32, LdtkTilesetInfo>,
    animations: &HashMap<LdtkTileKey, LdtkTileAnimation>,
) -> Vec<LdtkImportedEntity> {
    layer
        .entity_instances
        .iter()
        .map(|entity| {
            let tile = entity.tile.as_ref().map(|tile| {
                let tileset_uid = Some(tile.tileset_uid);
                let tileset = tilesets.get(&tile.tileset_uid);
                let tile_id = tileset
                    .and_then(|tileset| tile_id_from_rect(tile, tileset))
                    .unwrap_or_default();
                let key = LdtkTileKey {
                    tileset_uid,
                    tile_id,
                };

                LdtkTileMetadata {
                    level_identifier: level.identifier.clone(),
                    layer_identifier: layer.identifier.clone(),
                    layer_iid: layer.iid.clone(),
                    tileset_uid,
                    tileset_identifier: tileset
                        .map(|tileset| tileset.identifier.clone())
                        .unwrap_or_default(),
                    tile_id,
                    layer_position: entity.px,
                    source_position: IVec2::new(tile.x, tile.y),
                    rotation_degrees: 0,
                    flip_x: false,
                    flip_y: false,
                    alpha: 1.0,
                    custom_data: tileset
                        .and_then(|tileset| tileset.custom_data.get(&tile_id).cloned()),
                    tags: tileset
                        .and_then(|tileset| tileset.tile_tags.get(&tile_id).cloned())
                        .unwrap_or_default(),
                    animation: animations.get(&key).cloned(),
                }
            });

            LdtkImportedEntity {
                entity_iid: entity.iid.clone(),
                entity_identifier: entity.identifier.clone(),
                world_identifier: Some(world_identifier.to_string()),
                level_identifier: Some(level.identifier.clone()),
                layer_identifier: Some(layer.identifier.clone()),
                position: Vec2::new(entity.px.x as f32, entity.px.y as f32),
                grid_position: entity.grid,
                size: Vec2::new(entity.width as f32, entity.height as f32),
                pivot: entity.pivot,
                tags: entity.tags.clone(),
                tile,
                field_values: entity
                    .field_instances
                    .iter()
                    .map(|field| (field.identifier.clone(), LdtkFieldValue::from(field)))
                    .collect(),
            }
        })
        .collect()
}

fn tile_id_from_rect(rect: &TilesetRectangle, tileset: &LdtkTilesetInfo) -> Option<i32> {
    let stride = tileset.tile_grid_size + tileset.spacing;
    if stride <= 0 {
        return None;
    }

    let column = (rect.x - tileset.padding) / stride;
    let row = (rect.y - tileset.padding) / stride;
    if column < 0
        || row < 0
        || column >= tileset.grid_size_cells.x
        || row >= tileset.grid_size_cells.y
    {
        return None;
    }

    Some(row * tileset.grid_size_cells.x + column)
}

fn capture_collision_data(
    mut commands: Commands<'_, '_>,
    config: Res<'_, LdtkConfig>,
    mut catalog: ResMut<'_, LdtkCollisionCatalog>,
    mut load_state: ResMut<'_, LdtkLoadState>,
    int_grid_query: Query<
        '_,
        '_,
        (
            Entity,
            &IntGridCell,
            Option<&GridCoords>,
            Option<&LayerMetadata>,
        ),
        Added<IntGridCell>,
    >,
    layer_query: Query<'_, '_, &LayerMetadata, Added<LayerMetadata>>,
) {
    for layer in layer_query.iter() {
        let key = layer_key(&layer.level_id.to_string(), &layer.identifier);
        catalog
            .layers
            .entry(key)
            .or_insert_with(|| LdtkCollisionLayerInfo {
                level_identifier: layer.level_id.to_string(),
                layer_identifier: layer.identifier.clone(),
                layer_iid: layer.iid.clone(),
                layer_type: format!("{:?}", layer.layer_instance_type),
                solid_cells: 0,
                tile_cells: 0,
                sensor_cells: 0,
            });
    }

    for (entity, cell, grid, layer) in int_grid_query.iter() {
        let layer_identifier = layer
            .map(|metadata| metadata.identifier.clone())
            .unwrap_or_else(|| String::from("IntGrid"));
        let layer_iid = layer
            .map(|metadata| metadata.iid.clone())
            .unwrap_or_default();
        let level_identifier = layer
            .map(|metadata| metadata.level_id.to_string())
            .unwrap_or_default();
        let grid_position = grid
            .map(|coords| IVec2::new(coords.x, coords.y))
            .unwrap_or_default();
        let collision = resolve_collision_rule(&config, &layer_identifier, cell.value);

        catalog.cells.push(LdtkCollisionCell {
            level_identifier: level_identifier.clone(),
            layer_identifier: layer_identifier.clone(),
            layer_iid: layer_iid.clone(),
            grid_position,
            value: cell.value,
            solid: collision.solid,
            sensor: collision.sensor,
            tag: collision.tag.clone(),
        });

        if collision.solid || collision.sensor {
            commands.entity(entity).insert(LdtkCollider {
                solid: collision.solid,
                sensor: collision.sensor,
            });
        }

        let entry = catalog
            .layers
            .entry(layer_key(&level_identifier, &layer_identifier))
            .or_insert_with(|| LdtkCollisionLayerInfo {
                level_identifier,
                layer_identifier,
                layer_iid,
                layer_type: String::from("IntGrid"),
                solid_cells: 0,
                tile_cells: 0,
                sensor_cells: 0,
            });
        entry.solid_cells += usize::from(collision.solid);
        entry.sensor_cells += usize::from(collision.sensor);
        load_state.stats.collision_cells = catalog.cells.len();
    }
}

fn resolve_collision_rule(
    config: &LdtkConfig,
    layer_identifier: &str,
    value: i32,
) -> LdtkCollisionRule {
    if let Some(rule) = config.collision_rules.iter().find(|rule| {
        rule.value == value
            && rule
                .layer_identifier
                .as_deref()
                .map(|layer| layer == layer_identifier)
                .unwrap_or(true)
    }) {
        return rule.clone();
    }

    let solid = if config.int_grid_solid_values.is_empty() {
        value != 0
    } else {
        config.int_grid_solid_values.contains(&value)
    };

    LdtkCollisionRule {
        layer_identifier: Some(layer_identifier.to_string()),
        value,
        solid,
        sensor: false,
        tag: None,
    }
}

fn capture_entity_instances(
    mut commands: Commands<'_, '_>,
    mut entity_catalog: ResMut<'_, LdtkEntityCatalog>,
    map_catalog: Res<'_, LdtkMapCatalog>,
    query: Query<'_, '_, (Entity, &EntityInstance), Added<EntityInstance>>,
) {
    for (entity, instance) in query.iter() {
        let snapshot = map_catalog
            .levels
            .values()
            .flat_map(|level| level.entities.iter())
            .find(|snapshot| snapshot.entity_iid == instance.iid)
            .cloned()
            .unwrap_or_else(|| fallback_entity_snapshot(instance));

        entity_catalog
            .by_iid
            .insert(snapshot.entity_iid.clone(), entity);
        entity_catalog
            .snapshots
            .insert(snapshot.entity_iid.clone(), snapshot.clone());

        commands.entity(entity).insert((
            snapshot.clone(),
            LdtkEntityMarker {
                definition_identifier: snapshot.entity_identifier,
                level_identifier: snapshot.level_identifier,
                world_identifier: snapshot.world_identifier,
            },
        ));
    }
}

fn fallback_entity_snapshot(instance: &EntityInstance) -> LdtkImportedEntity {
    LdtkImportedEntity {
        entity_iid: instance.iid.clone(),
        entity_identifier: instance.identifier.clone(),
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
        ..Default::default()
    }
}

fn apply_registered_entity_behaviors(
    mut commands: Commands<'_, '_>,
    registry: Res<'_, LdtkEntityRegistry>,
    query: Query<'_, '_, (Entity, &LdtkImportedEntity), Added<LdtkImportedEntity>>,
) {
    for (entity, imported) in query.iter() {
        if registry
            .resolve(
                imported.layer_identifier.as_deref(),
                &imported.entity_identifier,
            )
            .is_none()
        {
            continue;
        }

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
            tile: imported.tile.clone(),
            field_values: imported.field_values.clone(),
        };
        let layer_identifier = imported.layer_identifier.clone();
        let entity_identifier = imported.entity_identifier.clone();

        commands.queue(move |world: &mut World| {
            world.resource_scope(|world, registry: Mut<'_, LdtkEntityRegistry>| {
                if let Some(spawner) =
                    registry.resolve(layer_identifier.as_deref(), &entity_identifier)
                {
                    spawner(world, entity, &context);
                }
            });
        });
    }
}

fn register_marked_entities(
    mut commands: Commands<'_, '_>,
    query: Query<'_, '_, Entity, Added<LdtkEntityMarker>>,
) {
    for entity in query.iter() {
        commands.entity(entity).insert(LdtkPersistent);
    }
}

fn sync_level_lifecycle_events(
    mut events: MessageReader<'_, '_, LevelEvent>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
) {
    for event in events.read() {
        match event {
            LevelEvent::Spawned(level_iid) => {
                runtime.loaded_levels.insert(level_iid.as_str().to_string());
            }
            LevelEvent::Despawned(level_iid) => {
                runtime.loaded_levels.remove(level_iid.as_str());
            }
            LevelEvent::SpawnTriggered(_) | LevelEvent::Transformed(_) => {}
        }
    }
}

fn tick_ldtk_tile_animators(time: Res<'_, Time>, mut query: Query<'_, '_, &mut LdtkTileAnimator>) {
    for mut animator in query.iter_mut() {
        animator.timer.tick(time.delta());
        if !animator.timer.just_finished() || animator.animation.frames.is_empty() {
            continue;
        }

        animator.frame_index += 1;
        if animator.frame_index >= animator.animation.frames.len() {
            animator.frame_index = if animator.animation.repeat {
                0
            } else {
                animator.animation.frames.len() - 1
            };
        }

        let duration = animator.animation.frames[animator.frame_index]
            .duration
            .max(0.001);
        animator.timer = Timer::from_seconds(duration, TimerMode::Repeating);
    }
}

fn parse_tile_animation(data: &str) -> Option<LdtkTileAnimation> {
    let normalized = data.trim();
    if normalized.is_empty() {
        return None;
    }

    let lower = normalized.to_lowercase();
    if !lower.contains("anim") && !lower.contains("frame") {
        return None;
    }

    let mut duration = 0.1;
    let mut repeat = true;
    let mut frame_text = normalized;

    for part in normalized.split(';') {
        let trimmed = part.trim();
        if let Some(value) = trimmed.strip_prefix("fps=") {
            if let Ok(fps) = value.trim().parse::<f32>() {
                duration = 1.0 / fps.max(0.001);
            }
        } else if let Some(value) = trimmed.strip_prefix("duration=") {
            if let Ok(seconds) = value.trim().parse::<f32>() {
                duration = seconds.max(0.001);
            }
        } else if let Some(value) = trimmed.strip_prefix("repeat=") {
            repeat = !matches!(value.trim(), "false" | "0" | "no");
        } else if trimmed.contains("anim") || trimmed.contains("frame") {
            frame_text = trimmed;
        }
    }

    let frame_text = frame_text
        .split_once('=')
        .map(|(_, value)| value)
        .or_else(|| frame_text.split_once(':').map(|(_, value)| value))
        .unwrap_or(frame_text);

    let frames = frame_text
        .split(',')
        .filter_map(|entry| {
            let entry = entry.trim();
            if entry.is_empty() {
                return None;
            }

            let (tile_id, frame_duration) = entry
                .split_once('@')
                .map(|(tile, seconds)| {
                    (
                        tile.trim().parse::<i32>().ok(),
                        seconds.trim().parse::<f32>().ok(),
                    )
                })
                .unwrap_or_else(|| (entry.parse::<i32>().ok(), None));

            tile_id.map(|tile_id| LdtkTileAnimationFrame {
                tile_id,
                duration: frame_duration.unwrap_or(duration).max(0.001),
            })
        })
        .collect::<Vec<_>>();

    (!frames.is_empty()).then_some(LdtkTileAnimation { frames, repeat })
}

fn tile_rotation_from_flags(flags: i32) -> u16 {
    match flags & 3 {
        0 | 1 => 0,
        2 | 3 => 180,
        _ => 0,
    }
}

fn layer_key(level_identifier: &str, layer_identifier: &str) -> String {
    format!("{level_identifier}:{layer_identifier}")
}

fn world_identifier_from_path(world_path: &str) -> String {
    Path::new(world_path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(world_path)
        .to_string()
}

fn world_layout_to_catalog(layout: WorldLayout) -> LdtkWorldLayout {
    match layout {
        WorldLayout::Free => LdtkWorldLayout::Free,
        WorldLayout::GridVania => LdtkWorldLayout::GridVania,
        WorldLayout::LinearHorizontal => LdtkWorldLayout::LinearHorizontal,
        WorldLayout::LinearVertical => LdtkWorldLayout::LinearVertical,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_animation_custom_data_with_fps() {
        let animation = parse_tile_animation("anim=1,2,3;fps=12").expect("animation");

        assert_eq!(animation.frames.len(), 3);
        assert_eq!(animation.frames[0].tile_id, 1);
        assert!((animation.frames[0].duration - (1.0 / 12.0)).abs() < f32::EPSILON);
        assert!(animation.repeat);
    }

    #[test]
    fn parses_animation_custom_data_with_frame_durations() {
        let animation =
            parse_tile_animation("frames=4@0.1,5@0.25;repeat=false").expect("animation");

        assert_eq!(animation.frames.len(), 2);
        assert_eq!(animation.frames[1].tile_id, 5);
        assert_eq!(animation.frames[1].duration, 0.25);
        assert!(!animation.repeat);
    }

    #[test]
    fn ignores_custom_data_without_animation_marker() {
        assert!(parse_tile_animation("solid=true").is_none());
    }

    #[test]
    fn resolves_tile_id_from_tileset_rect() {
        let tileset = LdtkTilesetInfo {
            tile_grid_size: 16,
            spacing: 0,
            padding: 0,
            grid_size_cells: IVec2::new(4, 4),
            ..Default::default()
        };
        let rect = TilesetRectangle {
            tileset_uid: 1,
            x: 32,
            y: 16,
            w: 16,
            h: 16,
        };

        assert_eq!(tile_id_from_rect(&rect, &tileset), Some(6));
    }

    #[test]
    fn builds_external_level_path_relative_to_world_file() {
        let path = external_level_path(
            "assets",
            "worlds/SeparateLevelFiles.ldtk",
            "SeparateLevelFiles/World_Level_0.ldtkl",
        );

        assert_eq!(
            path,
            PathBuf::from("assets")
                .join("worlds")
                .join("SeparateLevelFiles")
                .join("World_Level_0.ldtkl")
        );
    }

    #[test]
    fn collision_rules_override_default_solid_mapping() {
        let config = LdtkConfig::default().with_collision_rules([
            LdtkCollisionRule::sensor(2, "water").for_layer("Gameplay"),
            LdtkCollisionRule::solid(1),
        ]);

        let water = resolve_collision_rule(&config, "Gameplay", 2);
        assert!(!water.solid);
        assert!(water.sensor);
        assert_eq!(water.tag.as_deref(), Some("water"));

        let wall = resolve_collision_rule(&config, "Gameplay", 1);
        assert!(wall.solid);
        assert!(!wall.sensor);
    }
}
