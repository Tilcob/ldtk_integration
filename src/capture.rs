//! Runtime capture systems: mirror spawned IntGrid cells and entity instances
//! into the collision/entity catalogs and run registered spawner callbacks.

use bevy::prelude::*;
use bevy_ecs_ldtk::prelude::*;

use crate::catalog::{
    LdtkCollisionCatalog, LdtkCollisionCell, LdtkCollisionLayerInfo, LdtkEntityCatalog,
    LdtkLayerType, LdtkMapCatalog,
};
use crate::components::LdtkCollider;
use crate::config::{LdtkCollisionRule, LdtkConfig};
use crate::entities::{LdtkEntityMarker, LdtkEntitySpawnContext, LdtkImportedEntity};
use crate::fields::LdtkFieldValue;
use crate::registry::LdtkEntityRegistry;
use crate::state::{LdtkLoadState, LdtkRuntimeState};

pub(crate) fn capture_collision_data(
    mut commands: Commands<'_, '_>,
    config: Res<'_, LdtkConfig>,
    catalog: Res<'_, LdtkMapCatalog>,
    mut collision_catalog: ResMut<'_, LdtkCollisionCatalog>,
    mut load_state: ResMut<'_, LdtkLoadState>,
    int_grid_query: Query<'_, '_, (Entity, &IntGridCell, Option<&GridCoords>), Added<IntGridCell>>,
    layer_query: Query<'_, '_, &LayerMetadata, Added<LayerMetadata>>,
    metadata_query: Query<'_, '_, &LayerMetadata>,
    child_of_query: Query<'_, '_, &ChildOf>,
) {
    for layer in layer_query.iter() {
        let (level_identifier, level_iid) = resolve_level_reference(&catalog, layer);
        let key = layer_key(&level_identifier, &layer.identifier);
        collision_catalog
            .layers
            .entry(key)
            .or_insert_with(|| LdtkCollisionLayerInfo {
                level_identifier,
                level_iid,
                layer_identifier: layer.identifier.clone(),
                layer_iid: layer.iid.clone(),
                layer_type: LdtkLayerType::from(layer.layer_instance_type),
                solid_cells: 0,
                tile_cells: 0,
                sensor_cells: 0,
            });
    }

    let mut added_cells = false;
    for (entity, cell, grid) in int_grid_query.iter() {
        added_cells = true;
        // `bevy_ecs_ldtk` puts `LayerMetadata` on the layer entity, not on the
        // IntGrid cell entities (which are its children), so the metadata has
        // to be found by walking up the hierarchy.
        let layer = child_of_query
            .iter_ancestors(entity)
            .find_map(|ancestor| metadata_query.get(ancestor).ok());
        let layer_identifier = layer
            .map(|metadata| metadata.identifier.clone())
            .unwrap_or_else(|| String::from("IntGrid"));
        let layer_iid = layer
            .map(|metadata| metadata.iid.clone())
            .unwrap_or_default();
        let (level_identifier, level_iid) = layer
            .map(|metadata| resolve_level_reference(&catalog, metadata))
            .unwrap_or_else(|| (String::new(), String::new()));
        let grid_position = grid
            .map(|coords| IVec2::new(coords.x, coords.y))
            .unwrap_or_default();
        let collision = resolve_collision_rule(&config, &layer_identifier, cell.value);

        collision_catalog.cells.push(LdtkCollisionCell {
            level_identifier: level_identifier.clone(),
            level_iid: level_iid.clone(),
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

        let entry = collision_catalog
            .layers
            .entry(layer_key(&level_identifier, &layer_identifier))
            .or_insert_with(|| LdtkCollisionLayerInfo {
                level_identifier,
                level_iid,
                layer_identifier,
                layer_iid,
                layer_type: LdtkLayerType::IntGrid,
                solid_cells: 0,
                tile_cells: 0,
                sensor_cells: 0,
            });
        entry.solid_cells += usize::from(collision.solid);
        entry.sensor_cells += usize::from(collision.sensor);
    }

    // Update the counter once per batch, not once per cell: every write to a
    // ResMut field marks the resource changed and the per-cell version was pure
    // churn for change-detection consumers.
    if added_cells {
        load_state.stats.collision_cells = collision_catalog.cells.len();
    }
}

fn resolve_level_reference(catalog: &LdtkMapCatalog, layer: &LayerMetadata) -> (String, String) {
    // `LayerMetadata::level_id` is the numeric LDtk level **UID**, not an
    // identifier or iid string, so it resolves through the catalog's uid index.
    if let Some(level) = catalog.level_by_uid(layer.level_id) {
        return (level.identifier.clone(), level.iid.clone());
    }
    // Fall back to the layer's own iid: the catalog also records which level
    // each cataloged layer instance belongs to.
    if let Some(level) = catalog
        .layers
        .get(&layer.iid)
        .and_then(|info| catalog.level_by_id_or_iid(&info.level_identifier))
    {
        return (level.identifier.clone(), level.iid.clone());
    }
    (String::new(), String::new())
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

pub(crate) fn capture_entity_instances(
    mut commands: Commands<'_, '_>,
    mut entity_catalog: ResMut<'_, LdtkEntityCatalog>,
    map_catalog: Res<'_, LdtkMapCatalog>,
    query: Query<'_, '_, (Entity, &EntityInstance), Added<EntityInstance>>,
) {
    for (entity, instance) in query.iter() {
        // O(1) via the catalog's entity-iid index instead of scanning every
        // entity of every level per spawned instance.
        let snapshot = match map_catalog.entity_snapshot_by_iid(&instance.iid).cloned() {
            Some(snapshot) => snapshot,
            None => {
                warn!(
                    "LDtk entity '{}' ({}) not found in the map catalog; using a fallback snapshot without world/level/layer context. \
                     This usually means the entity's layer was filtered out or the catalog has not been built yet.",
                    instance.identifier, instance.iid
                );
                fallback_entity_snapshot(instance)
            }
        };

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

pub(crate) fn apply_registered_entity_behaviors(
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

pub(crate) fn sync_level_lifecycle_events(
    mut events: MessageReader<'_, '_, LevelEvent>,
    mut runtime: ResMut<'_, LdtkRuntimeState>,
    catalog: Res<'_, LdtkMapCatalog>,
    mut collision_catalog: ResMut<'_, LdtkCollisionCatalog>,
    mut entity_catalog: ResMut<'_, LdtkEntityCatalog>,
) {
    for event in events.read() {
        match event {
            LevelEvent::Spawned(level_iid) => {
                runtime.loaded_levels.insert(level_iid.as_str().to_string());
            }
            LevelEvent::Despawned(level_iid) => {
                let iid = level_iid.as_str();
                runtime.loaded_levels.remove(iid);

                // Drop this level's catalog data so neighbor streaming
                // (load/unload of adjacent levels) cannot accumulate stale
                // colliders or dangling entity handles.
                collision_catalog.cells.retain(|cell| cell.level_iid != iid);
                collision_catalog
                    .layers
                    .retain(|_, info| info.level_iid != iid);

                if let Some(identifier) = catalog.identifier_for_iid(iid) {
                    // Split-borrow the catalog so `by_iid` can be filtered
                    // against `snapshots` directly, without materialising an
                    // intermediate set of live keys.
                    let LdtkEntityCatalog { by_iid, snapshots } = &mut *entity_catalog;
                    snapshots.retain(|_, snapshot| {
                        snapshot.level_identifier.as_deref() != Some(identifier)
                    });
                    by_iid.retain(|entity_iid, _| snapshots.contains_key(entity_iid));
                }
            }
            LevelEvent::SpawnTriggered(_) | LevelEvent::Transformed(_) => {}
        }
    }
}

/// Composite key for [`LdtkCollisionCatalog::layers`].
fn layer_key(level_identifier: &str, layer_identifier: &str) -> String {
    format!("{level_identifier}:{layer_identifier}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::LdtkLevelInfo;

    /// Regression test: `LayerMetadata` lives on the parent layer entity (not
    /// on the IntGrid cell entities) and its `level_id` is the numeric level
    /// UID, so captured cells used to end up with empty level/layer references.
    #[test]
    fn captured_cells_carry_level_and_layer_from_parent_layer_entity() {
        let mut world = World::new();

        let mut catalog = LdtkMapCatalog::default();
        catalog.insert_level_info(LdtkLevelInfo {
            iid: "level-iid".to_string(),
            uid: 42,
            identifier: "Level_A".to_string(),
            ..Default::default()
        });
        world.insert_resource(catalog);
        world.insert_resource(LdtkConfig::default());
        world.insert_resource(LdtkCollisionCatalog::default());
        world.insert_resource(LdtkLoadState::default());

        let layer_entity = world
            .spawn(LayerMetadata {
                identifier: "Collision".to_string(),
                iid: "layer-iid".to_string(),
                level_id: 42,
                layer_instance_type: bevy_ecs_ldtk::ldtk::Type::IntGrid,
                ..Default::default()
            })
            .id();
        let cell_entity = world
            .spawn((
                IntGridCell { value: 1 },
                GridCoords { x: 3, y: 5 },
                ChildOf(layer_entity),
            ))
            .id();
        // Cells nested deeper in the hierarchy must resolve too.
        let intermediate = world.spawn(ChildOf(layer_entity)).id();
        world.spawn((
            IntGridCell { value: 1 },
            GridCoords { x: 0, y: 0 },
            ChildOf(intermediate),
        ));

        let mut schedule = Schedule::default();
        schedule.add_systems(capture_collision_data);
        schedule.run(&mut world);

        let collision = world.resource::<LdtkCollisionCatalog>();
        assert_eq!(collision.cells.len(), 2);
        for cell in &collision.cells {
            assert_eq!(cell.level_identifier, "Level_A");
            assert_eq!(cell.level_iid, "level-iid");
            assert_eq!(cell.layer_identifier, "Collision");
            assert_eq!(cell.layer_iid, "layer-iid");
            assert!(cell.solid);
        }
        assert_eq!(collision.cells[0].grid_position, IVec2::new(3, 5));

        let layer_info = collision
            .layers
            .get("Level_A:Collision")
            .expect("layer summary keyed by resolved level identifier");
        assert_eq!(layer_info.level_iid, "level-iid");
        assert_eq!(layer_info.layer_iid, "layer-iid");
        assert_eq!(layer_info.solid_cells, 2);

        // Solid cells get an LdtkCollider via deferred commands.
        assert!(world.entity(cell_entity).contains::<LdtkCollider>());
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
