//! In-memory catalogs of everything parsed from the world JSON: worlds, levels,
//! layers, tilesets, tiles, spawn points, collision cells, and entity snapshots.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::animation::{LdtkTileAnimation, LdtkTileKey};
use crate::entities::LdtkImportedEntity;
use crate::fields::LdtkFieldValue;

/// Bevy resource acting as the in-memory catalog of everything parsed from the
/// world JSON, keyed for fast O(1) look-ups.
#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkMapCatalog {
    /// Worlds indexed by their LDtk `identifier`.
    pub worlds: HashMap<String, LdtkWorldInfo>,
    /// Levels keyed by their LDtk `identifier`.
    pub levels: HashMap<String, LdtkLevelInfo>,
    /// Secondary index mapping a level `iid` to its `identifier`, so lookups by
    /// iid are O(1) instead of a linear scan over [`Self::levels`]. Kept in sync
    /// by [`Self::insert_level_info`].
    pub levels_by_iid: HashMap<String, String>,
    /// Secondary index mapping a level's numeric LDtk `uid` to its `identifier`.
    /// `bevy_ecs_ldtk`'s `LayerMetadata::level_id` references levels by uid, so
    /// runtime capture needs this to attribute layers/cells to a level. Kept in
    /// sync by [`Self::insert_level_info`].
    pub levels_by_uid: HashMap<i32, String>,
    /// Layer instances keyed by their LDtk instance **IID** (identifiers are not
    /// unique across levels, IIDs are).
    pub layers: HashMap<String, LdtkLayerInfo>,
    /// Tilesets indexed by their numeric LDtk UID.
    pub tilesets: HashMap<i32, LdtkTilesetInfo>,
    /// Per-tile animation definitions indexed by [`LdtkTileKey`].
    pub tile_animations: HashMap<LdtkTileKey, LdtkTileAnimation>,
    /// Secondary index mapping an entity `iid` to its level `identifier` and its
    /// position in that level's [`LdtkLevelInfo::entities`] list, so snapshot
    /// lookups are O(1) instead of a scan over every entity of every level.
    /// Kept in sync by [`Self::insert_level_info`].
    pub entities_by_iid: HashMap<String, (String, usize)>,
}

impl LdtkMapCatalog {
    /// Returns `true` when neither worlds nor levels have been cataloged yet.
    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty() && self.levels.is_empty()
    }

    /// Inserts a level while keeping the `iid -> identifier`,
    /// `uid -> identifier`, and `entity iid -> snapshot` indexes in sync.
    pub fn insert_level_info(&mut self, info: LdtkLevelInfo) {
        self.levels_by_iid
            .insert(info.iid.clone(), info.identifier.clone());
        self.levels_by_uid.insert(info.uid, info.identifier.clone());
        for (index, entity) in info.entities.iter().enumerate() {
            self.entities_by_iid
                .insert(entity.entity_iid.clone(), (info.identifier.clone(), index));
        }
        self.levels.insert(info.identifier.clone(), info);
    }

    /// Removes all cataloged data, including the secondary indexes.
    pub fn clear(&mut self) {
        self.worlds.clear();
        self.levels.clear();
        self.levels_by_iid.clear();
        self.levels_by_uid.clear();
        self.layers.clear();
        self.tilesets.clear();
        self.tile_animations.clear();
        self.entities_by_iid.clear();
    }

    /// Resolves an `iid` to the level `identifier` in O(1).
    pub fn identifier_for_iid(&self, iid: &str) -> Option<&str> {
        self.levels_by_iid.get(iid).map(String::as_str)
    }

    /// Looks up a level by either its `identifier` or its `iid`.
    pub fn level_by_id_or_iid(&self, id: &str) -> Option<&LdtkLevelInfo> {
        if let Some(level) = self.levels.get(id) {
            return Some(level);
        }
        self.levels_by_iid
            .get(id)
            .and_then(|identifier| self.levels.get(identifier))
    }

    /// Looks up a level by its numeric LDtk `uid` in O(1) via
    /// [`Self::levels_by_uid`].
    pub fn level_by_uid(&self, uid: i32) -> Option<&LdtkLevelInfo> {
        self.levels_by_uid
            .get(&uid)
            .and_then(|identifier| self.levels.get(identifier))
    }

    /// Looks up an entity snapshot by its instance `iid` in O(1) via
    /// [`Self::entities_by_iid`].
    pub fn entity_snapshot_by_iid(&self, iid: &str) -> Option<&LdtkImportedEntity> {
        let (level_identifier, index) = self.entities_by_iid.get(iid)?;
        self.levels
            .get(level_identifier)?
            .entities
            .get(*index)
            // Guard against a stale index entry (e.g. a level replaced without
            // a full catalog rebuild).
            .filter(|snapshot| snapshot.entity_iid == iid)
    }
}

/// Bevy resource containing all collision layers and cells extracted during
/// catalog construction.
#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkCollisionCatalog {
    /// Per-layer collision summaries, keyed by `"<level identifier>:<layer identifier>"`.
    pub layers: HashMap<String, LdtkCollisionLayerInfo>,
    /// Flat list of every individual collision cell across all layers and levels.
    pub cells: Vec<LdtkCollisionCell>,
}

impl LdtkCollisionCatalog {
    /// World-space center of a collision cell, in the same Bevy y-up coordinate
    /// space as rendered tiles and [`LdtkSpawnPoint::position`].
    ///
    /// Needs the [`LdtkMapCatalog`] to look up the cell's level (world origin and
    /// height) and layer (grid size). Returns `None` when either is missing or
    /// the layer has a non-positive grid size.
    ///
    /// `GridCoords` are y-up with the origin at the level's bottom-left, so with
    /// the level's Bevy origin at `(world_x, -world_y - px_hei)` the cell center
    /// is `origin + ((gx + 0.5) * grid, (gy + 0.5) * grid)`. This mirrors the
    /// pixel→world conversion used for spawn points, so colliders, tiles, and
    /// gameplay distances all share one convention.
    pub fn cell_world_center(
        &self,
        cell: &LdtkCollisionCell,
        map: &LdtkMapCatalog,
    ) -> Option<Vec2> {
        let level = map.level_by_id_or_iid(&cell.level_identifier)?;
        let layer = map.layers.get(&cell.layer_iid)?;
        if layer.grid_size <= 0 {
            return None;
        }

        let grid = layer.grid_size as f32;
        let origin = Vec2::new(
            level.world_position.x as f32,
            -(level.world_position.y as f32) - level.size.y as f32,
        );
        Some(
            origin
                + Vec2::new(
                    (cell.grid_position.x as f32 + 0.5) * grid,
                    (cell.grid_position.y as f32 + 0.5) * grid,
                ),
        )
    }
}

/// Bevy resource that provides O(1) look-ups from LDtk entity IIDs to their
/// Bevy [`Entity`] handles, plus full snapshots of each imported entity.
#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkEntityCatalog {
    /// Maps entity IIDs to their corresponding Bevy [`Entity`].
    pub by_iid: HashMap<String, Entity>,
    /// Full [`LdtkImportedEntity`] snapshots keyed by entity IID.
    pub snapshots: HashMap<String, LdtkImportedEntity>,
}

/// Strongly typed mirror of `bevy_ecs_ldtk`'s layer kind, so consumers match on
/// a stable enum instead of a `format!("{:?}", ...)` debug string that silently
/// breaks if the upstream `Debug` impl changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LdtkLayerType {
    /// An IntGrid layer storing integer values per cell.
    IntGrid,
    /// A layer containing entity instances.
    Entities,
    /// A manually placed tile layer.
    Tiles,
    /// An auto-tile layer driven by rules.
    AutoLayer,
    /// The source layer type could not be mapped to a known variant.
    #[default]
    Unknown,
}

impl From<bevy_ecs_ldtk::ldtk::Type> for LdtkLayerType {
    fn from(value: bevy_ecs_ldtk::ldtk::Type) -> Self {
        use bevy_ecs_ldtk::ldtk::Type;
        match value {
            Type::IntGrid => Self::IntGrid,
            Type::Entities => Self::Entities,
            Type::Tiles => Self::Tiles,
            Type::AutoLayer => Self::AutoLayer,
        }
    }
}

/// Summary of collision data extracted from a single layer instance inside a
/// level.
#[derive(Debug, Clone, Default)]
pub struct LdtkCollisionLayerInfo {
    /// LDtk identifier of the level this layer belongs to.
    pub level_identifier: String,
    /// LDtk IID of the level this layer belongs to.
    pub level_iid: String,
    /// LDtk identifier of this layer definition.
    pub layer_identifier: String,
    /// Instance-unique IID for this layer.
    pub layer_iid: String,
    /// Kind of layer (IntGrid, Tiles, etc.).
    pub layer_type: LdtkLayerType,
    /// Number of solid (impassable) collision cells in this layer.
    pub solid_cells: usize,
    /// Number of tile-backed collision cells in this layer.
    pub tile_cells: usize,
    /// Number of sensor (trigger) collision cells in this layer.
    pub sensor_cells: usize,
}

/// Collision data for a single IntGrid cell, produced during catalog construction
/// and stored in [`LdtkCollisionCatalog::cells`].
#[derive(Debug, Clone, Default)]
pub struct LdtkCollisionCell {
    /// LDtk identifier of the level that contains this cell.
    pub level_identifier: String,
    /// LDtk IID of the containing level.
    pub level_iid: String,
    /// LDtk identifier of the layer that contains this cell.
    pub layer_identifier: String,
    /// Instance-unique IID of the containing layer.
    pub layer_iid: String,
    /// Column/row position of this cell within the layer grid (in grid units).
    pub grid_position: IVec2,
    /// The raw IntGrid value stored in this cell.
    pub value: i32,
    /// Whether this cell acts as a solid (impassable) collider.
    pub solid: bool,
    /// Whether this cell acts as a sensor (trigger) collider.
    pub sensor: bool,
    /// Optional tag identifying the sensor type (e.g. `"water"`, `"spike"`).
    pub tag: Option<String>,
}

/// Metadata about a single LDtk world, populated during catalog construction.
#[derive(Debug, Clone, Default)]
pub struct LdtkWorldInfo {
    /// LDtk identifier for this world.
    pub identifier: String,
    /// Asset-relative path to the `.ldtk` file.
    pub path: String,
    /// Identifiers of all levels that belong to this world.
    pub levels: Vec<String>,
    /// Spatial layout strategy used by this world.
    pub layout: LdtkWorldLayout,
}

/// Metadata about a single LDtk level.
#[derive(Debug, Clone, Default)]
pub struct LdtkLevelInfo {
    /// Instance-unique identifier for this level.
    pub iid: String,
    /// Numeric LDtk UID for this level; `LayerMetadata::level_id` references
    /// levels by this value at runtime.
    pub uid: i32,
    /// Human-readable LDtk identifier for this level.
    pub identifier: String,
    /// Identifier of the world this level belongs to.
    pub world_identifier: String,
    /// Asset-relative path to the external `.ldtkl` file, if the level is saved separately.
    pub external_path: Option<String>,
    /// Width and height of the level in pixels.
    pub size: IVec2,
    /// Top-left position of this level in world-space pixels.
    pub world_position: IVec2,
    /// Adjacent levels and their directions, used for neighbor-loading.
    pub neighbors: Vec<LdtkNeighbor>,
    /// Spawn points extracted from entity instances inside this level.
    pub spawn_points: Vec<LdtkSpawnPoint>,
    /// Tile metadata for every tile instance in this level.
    pub tiles: Vec<LdtkTileMetadata>,
    /// Imported entity instances in this level.
    pub entities: Vec<LdtkImportedEntity>,
    /// Custom field values defined on this level in the LDtk editor.
    pub fields: HashMap<String, LdtkFieldValue>,
}

/// Metadata about a single layer definition instance within a level.
#[derive(Debug, Clone, Default)]
pub struct LdtkLayerInfo {
    /// Instance-unique IID for this layer.
    pub iid: String,
    /// LDtk identifier of the layer definition.
    pub identifier: String,
    /// Identifier of the level that contains this layer.
    pub level_identifier: String,
    /// Kind of layer (IntGrid, Entities, Tiles, or AutoLayer).
    pub layer_type: LdtkLayerType,
    /// Size of each cell in pixels.
    pub grid_size: i32,
    /// Dimensions of the grid in cells (columns × rows).
    pub grid_size_cells: IVec2,
    /// UID of the tileset used by this layer, if any.
    pub tileset_uid: Option<i32>,
    /// Asset-relative path to the tileset image file, if any.
    pub tileset_rel_path: Option<String>,
    /// Layer opacity in the range `0.0` (transparent) to `1.0` (opaque).
    pub opacity: f32,
    /// Whether the layer was marked as visible in the LDtk editor.
    pub visible: bool,
}

/// Metadata about a tileset definition.
#[derive(Debug, Clone, Default)]
pub struct LdtkTilesetInfo {
    /// LDtk numeric UID for this tileset.
    pub uid: i32,
    /// LDtk identifier for this tileset.
    pub identifier: String,
    /// Asset-relative path to the tileset image, or `None` for internal tilesets.
    pub rel_path: Option<String>,
    /// Size of each tile in pixels (tiles are assumed square).
    pub tile_grid_size: i32,
    /// Dimensions of the tileset in tiles (columns × rows).
    pub grid_size_cells: IVec2,
    /// Full pixel dimensions of the tileset image.
    pub image_size: IVec2,
    /// Pixel gap between tiles in the image.
    pub spacing: i32,
    /// Pixel border around the tileset image edge.
    pub padding: i32,
    /// Tags assigned to the entire tileset.
    pub tags: Vec<String>,
    /// Per-tile tags, keyed by tile ID.
    pub tile_tags: HashMap<i32, Vec<String>>,
    /// Per-tile custom data strings, keyed by tile ID.
    pub custom_data: HashMap<i32, String>,
}

/// Reference from a level to one of its adjacent levels, used for neighbor-based
/// level streaming.
#[derive(Debug, Clone, Default)]
pub struct LdtkNeighbor {
    /// LDtk identifier of the adjacent level.
    pub level_identifier: String,
    /// Cardinal direction from the owning level to this neighbor.
    pub direction: LdtkDirection,
    /// Optional traversal cost for graph-based pathfinding over levels; `1.0` by default.
    pub cost: f32,
}

/// Cardinal direction used in [`LdtkNeighbor`] and level-adjacency queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LdtkDirection {
    /// The neighboring level is above (decreasing Y in world space).
    #[default]
    North,
    /// The neighboring level is below (increasing Y in world space).
    South,
    /// The neighboring level is to the right (increasing X in world space).
    East,
    /// The neighboring level is to the left (decreasing X in world space).
    West,
}

/// Spatial arrangement strategy for the levels in a world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LdtkWorldLayout {
    /// Levels are placed freely at arbitrary positions.
    #[default]
    Free,
    /// Levels form a contiguous grid where neighbors share borders.
    GridVania,
    /// Levels are arranged in a single horizontal row.
    LinearHorizontal,
    /// Levels are arranged in a single vertical column.
    LinearVertical,
}

/// A named location inside a level from which a player or object can be placed.
#[derive(Debug, Clone, Default)]
pub struct LdtkSpawnPoint {
    /// LDtk entity identifier used as the spawn-point type (e.g. `"PlayerStart"`).
    pub identifier: String,
    /// World-space position of the spawn point in pixels.
    pub position: Vec2,
    /// Identifier of the level that contains this spawn point.
    pub level_identifier: String,
    /// Identifier of the layer the spawn-point entity lives on.
    pub layer_identifier: String,
    /// Tags copied from the LDtk entity instance for filtering.
    pub tags: Vec<String>,
}

/// Metadata for a single tile instance placed in a level, including flip flags,
/// custom data, and an optional animation definition.
#[derive(Debug, Clone, Default)]
pub struct LdtkTileMetadata {
    /// Identifier of the level that contains this tile.
    pub level_identifier: String,
    /// Identifier of the layer that contains this tile.
    pub layer_identifier: String,
    /// Instance IID of the layer that contains this tile.
    pub layer_iid: String,
    /// UID of the tileset this tile's graphic comes from.
    pub tileset_uid: Option<i32>,
    /// Identifier of the tileset this tile's graphic comes from.
    pub tileset_identifier: String,
    /// Index of this tile's graphic within its tileset.
    pub tile_id: i32,
    /// Position of the tile's top-left corner inside the layer, in **pixels**
    /// (LDtk's `px`). Divide by [`LdtkLayerInfo::grid_size`] for grid coordinates.
    pub layer_position: IVec2,
    /// Pixel offset of the top-left corner of this tile's source rectangle in the tileset image.
    pub source_position: IVec2,
    // LDtk tiles carry no native rotation, only flip flags. A 180° rotation is
    // simply `flip_x && flip_y`; consumers derive that themselves instead of us
    // shipping a redundant (and previously incorrect) `rotation_degrees` field.
    /// Whether the tile is flipped horizontally.
    pub flip_x: bool,
    /// Whether the tile is flipped vertically.
    pub flip_y: bool,
    /// Opacity of this tile instance in the range `0.0` (transparent) to `1.0` (opaque).
    pub alpha: f32,
    /// Custom data string attached to this tile ID in the tileset, if any.
    pub custom_data: Option<String>,
    /// Tags inherited from the tileset definition for this tile ID.
    pub tags: Vec<String>,
    /// Frame sequence for animated tiles; `None` for static tiles.
    pub animation: Option<LdtkTileAnimation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collision_cell_world_center_matches_spawn_convention() {
        let mut map = LdtkMapCatalog::default();
        map.insert_level_info(LdtkLevelInfo {
            iid: "lvl-iid".to_string(),
            identifier: "Level_A".to_string(),
            size: IVec2::new(256, 128),
            world_position: IVec2::new(0, 0),
            ..Default::default()
        });
        map.layers.insert(
            "layer-iid".to_string(),
            LdtkLayerInfo {
                iid: "layer-iid".to_string(),
                grid_size: 16,
                ..Default::default()
            },
        );

        let catalog = LdtkCollisionCatalog::default();
        let cell = LdtkCollisionCell {
            level_identifier: "Level_A".to_string(),
            layer_iid: "layer-iid".to_string(),
            grid_position: IVec2::new(0, 0),
            ..Default::default()
        };

        // Level origin (bottom-left) is (0, -0 - 128) = (0, -128); the bottom-left
        // cell center sits half a grid cell in from there.
        assert_eq!(
            catalog.cell_world_center(&cell, &map),
            Some(Vec2::new(8.0, -120.0))
        );

        // A cell whose layer is not in the catalog resolves to None.
        let orphan = LdtkCollisionCell {
            level_identifier: "Level_A".to_string(),
            layer_iid: "missing".to_string(),
            ..Default::default()
        };
        assert_eq!(catalog.cell_world_center(&orphan, &map), None);
    }

    #[test]
    fn catalog_resolves_levels_by_identifier_and_iid() {
        let mut catalog = LdtkMapCatalog::default();
        let info = LdtkLevelInfo {
            iid: "abc-123".to_string(),
            identifier: "Level_A".to_string(),
            ..Default::default()
        };
        catalog.insert_level_info(info);

        assert_eq!(catalog.identifier_for_iid("abc-123"), Some("Level_A"));
        assert_eq!(
            catalog
                .level_by_id_or_iid("Level_A")
                .map(|l| l.iid.as_str()),
            Some("abc-123")
        );
        assert_eq!(
            catalog
                .level_by_id_or_iid("abc-123")
                .map(|l| l.identifier.as_str()),
            Some("Level_A")
        );
        assert!(catalog.level_by_id_or_iid("missing").is_none());
    }

    #[test]
    fn catalog_resolves_entity_snapshots_by_iid() {
        let mut catalog = LdtkMapCatalog::default();
        catalog.insert_level_info(LdtkLevelInfo {
            iid: "lvl-iid".to_string(),
            identifier: "Level_A".to_string(),
            entities: vec![
                LdtkImportedEntity {
                    entity_iid: "ent-1".to_string(),
                    entity_identifier: "Door".to_string(),
                    ..Default::default()
                },
                LdtkImportedEntity {
                    entity_iid: "ent-2".to_string(),
                    entity_identifier: "Chest".to_string(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        assert_eq!(
            catalog
                .entity_snapshot_by_iid("ent-2")
                .map(|e| e.entity_identifier.as_str()),
            Some("Chest")
        );
        assert!(catalog.entity_snapshot_by_iid("missing").is_none());

        catalog.clear();
        assert!(catalog.entity_snapshot_by_iid("ent-2").is_none());
    }
}
