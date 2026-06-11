//! Builds the [`LdtkMapCatalog`] from the loaded `LdtkProject` JSON: levels,
//! layers, tilesets, tiles, spawn points, entity snapshots, and neighbor graphs.

use bevy::prelude::*;
use bevy_ecs_ldtk::ldtk::{
    LayerInstance, LdtkJson, Level, TileInstance, TilesetDefinition, TilesetRectangle, WorldLayout,
};
use bevy_ecs_ldtk::prelude::*;
use std::collections::HashMap;

use crate::animation::{LdtkTileAnimation, LdtkTileKey, parse_tile_animation};
use crate::catalog::{
    LdtkDirection, LdtkLayerInfo, LdtkLayerType, LdtkLevelInfo, LdtkMapCatalog, LdtkNeighbor,
    LdtkSpawnPoint, LdtkTileMetadata, LdtkTilesetInfo, LdtkWorldInfo, LdtkWorldLayout,
};
use crate::config::LdtkConfig;
use crate::entities::LdtkImportedEntity;
use crate::events::{LdtkMapLoadedEvent, LdtkValidationFinishedEvent};
use crate::external::{ExternalLevelSource, LdtkExternalLevelSource};
use crate::fields::LdtkFieldValue;
use crate::plugin::world_identifier_from_path;
use crate::registry::LdtkEntityRegistry;
use crate::state::{
    LdtkLoadState, LdtkLoadStats, LdtkLoadStatus, LdtkRuntimeState, LdtkTransitionState,
    LdtkValidationReport,
};
use crate::validation::validate_catalog;

pub(crate) fn refresh_map_catalog_from_project(
    project_assets: Res<'_, Assets<LdtkProject>>,
    world_query: Query<'_, '_, &LdtkProjectHandle, With<crate::components::LdtkWorldRoot>>,
    mut catalog: ResMut<'_, LdtkMapCatalog>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
    registry: Res<'_, LdtkEntityRegistry>,
    config: Res<'_, LdtkConfig>,
    external_source: Res<'_, LdtkExternalLevelSource>,
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

    // The catalog is a pure projection of the loaded LdtkProject asset.
    // Only rebuild it when the asset collection actually changed (initial
    // load or reload); otherwise we would clear and repopulate every frame.
    if !project_assets.is_changed() && !catalog.is_empty() {
        return;
    }

    let json = project.json_data();
    let active_world_path = runtime.active_world_path.clone().unwrap_or_default();
    let external_source = external_source.source();

    catalog.clear();
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
            let level =
                level_with_external_data(level, &active_world_path, &config, external_source)
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
                    layout,
                },
            );

            for level in &world.levels {
                let level =
                    level_with_external_data(level, &active_world_path, &config, external_source)
                        .unwrap_or_else(|| level.clone());
                insert_level(&level, &world.identifier, layout, &mut catalog, &config);
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

            info.spawn_points.extend(extract_spawn_points(level, layer));
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

    catalog.insert_level_info(info);
}

/// Pulls in the layer data of an external `.ldtkl` level via the injected
/// [`ExternalLevelSource`]. Returns `None` when external cataloging is disabled,
/// the level is already embedded, or no source is available (e.g. WASM).
fn level_with_external_data(
    level: &Level,
    active_world_path: &str,
    config: &LdtkConfig,
    source: Option<&dyn ExternalLevelSource>,
) -> Option<Level> {
    if !config.catalog_external_levels || level.layer_instances.is_some() {
        return None;
    }

    let external_path = level.external_rel_path.as_ref()?;
    let text = source?.load(&config.asset_root, active_world_path, external_path)?;
    serde_json::from_str::<Level>(&text).ok()
}

fn level_info_from_level(level: &Level, world_identifier: &str) -> LdtkLevelInfo {
    LdtkLevelInfo {
        iid: level.iid.clone(),
        uid: level.uid,
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
        layer_type: LdtkLayerType::from(layer.layer_instance_type),
        grid_size: layer.grid_size,
        grid_size_cells: IVec2::new(layer.c_wid, layer.c_hei),
        tileset_uid: layer.override_tileset_uid.or(layer.tileset_def_uid),
        tileset_rel_path: layer.tileset_rel_path.clone(),
        opacity: layer.opacity,
        visible: layer.visible,
    }
}

/// Refreshes the catalog-derived counters on [`LdtkLoadState`].
/// `collision_cells` is intentionally carried over: it is owned by the
/// `Added<IntGridCell>` capture pass, not by the catalog rebuild, so zeroing it
/// here would desync it from `LdtkCollisionCatalog::cells` on hot reloads.
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
        collision_cells: load_state.stats.collision_cells,
        tile_animations: catalog.tile_animations.len(),
    };
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

/// Heuristic traversal cost for an overlapping neighbor (LDtk `dir` value `"o"`):
/// the levels share space, so "moving" there is nearly free.
const NEIGHBOR_COST_OVERLAP: f32 = 0.2;
/// Heuristic traversal cost for depth neighbors (LDtk `dir` values `"<"`/`">"`,
/// i.e. levels on a lower/higher world depth): crossing depths is assumed to be
/// more expensive than walking to an adjacent level.
const NEIGHBOR_COST_DEPTH: f32 = 1.5;
/// Heuristic traversal cost for diagonal neighbors (two-letter LDtk `dir` values
/// such as `"ne"`), approximating sqrt(2).
const NEIGHBOR_COST_DIAGONAL: f32 = 1.4;
/// Default traversal cost for cardinal neighbors.
const NEIGHBOR_COST_CARDINAL: f32 = 1.0;

/// Maps an LDtk neighbor `dir` string to a relative traversal cost for
/// graph-based pathfinding over levels. The values are heuristics, not physics:
/// consumers that need exact costs should derive their own from level geometry.
fn neighbor_cost(value: &str) -> f32 {
    match value {
        "o" => NEIGHBOR_COST_OVERLAP,
        "<" | ">" => NEIGHBOR_COST_DEPTH,
        diagonal if diagonal.len() == 2 => NEIGHBOR_COST_DIAGONAL,
        _ => NEIGHBOR_COST_CARDINAL,
    }
}

/// Derives a direction from the relative world positions of two levels.
/// Levels whose world position is entirely unset (LDtk writes `-1/-1` for
/// layouts without world coordinates) yield `None`; a single `-1` coordinate is
/// treated as a legitimate position.
fn direction_by_world_position(source: &Level, target: &Level) -> Option<LdtkDirection> {
    let unset = |level: &Level| level.world_x == -1 && level.world_y == -1;
    if unset(source) || unset(target) {
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

/// Converts an LDtk pixel coordinate (relative to the level's top-left, y-down)
/// into a Bevy world position (y-up), matching how `bevy_ecs_ldtk` places levels
/// under `LevelSpawnBehavior::UseWorldTranslation`.
///
/// LDtk stores entity `px` and a level's `world_x`/`world_y` y-down; Bevy renders
/// y-up. A level's Bevy origin (its bottom-left corner) sits at
/// `(world_x, -world_y - px_hei)`, and a point `px` inside the level maps to the
/// level-local offset `(px.x, px_hei - px.y)`. Summing the two collapses the
/// level height out entirely, leaving `(world_x + px.x, -(world_y + px.y))`, so a
/// player teleported here lands exactly on the rendered entity.
fn ldtk_px_to_world(px: IVec2, level_world: IVec2) -> Vec2 {
    Vec2::new(
        (level_world.x + px.x) as f32,
        -((level_world.y + px.y) as f32),
    )
}

/// Extracts spawn points from an entity layer. An entity counts as a spawn point
/// when its identifier contains `"spawn"` (case-insensitive) or it carries a
/// `spawn` tag. Tags are copied verbatim from the LDtk entity instance.
fn extract_spawn_points(level: &Level, layer: &LayerInstance) -> Vec<LdtkSpawnPoint> {
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
        .map(|entity| LdtkSpawnPoint {
            identifier: entity.identifier.clone(),
            position: ldtk_px_to_world(entity.px, IVec2::new(level.world_x, level.world_y)),
            level_identifier: level.identifier.clone(),
            layer_identifier: layer.identifier.clone(),
            tags: entity.tags.clone(),
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
                position: ldtk_px_to_world(entity.px, IVec2::new(level.world_x, level.world_y)),
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
    fn converts_ldtk_pixels_to_bevy_world_y_up() {
        // Level at the world origin: x is unchanged, y is simply flipped.
        assert_eq!(
            ldtk_px_to_world(IVec2::new(32, 48), IVec2::ZERO),
            Vec2::new(32.0, -48.0)
        );
        // Level offset in world space: both axes shift, y stays flipped.
        assert_eq!(
            ldtk_px_to_world(IVec2::new(10, 20), IVec2::new(256, 128)),
            Vec2::new(266.0, -148.0)
        );
    }
}
