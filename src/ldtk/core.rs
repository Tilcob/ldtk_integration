use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Explicit ordering for the LDtk systems so dependent stages run in a
/// deterministic sequence instead of relying on tuple insertion order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum LdtkLoadSet {
    /// Processes queued [`LdtkCommand`]s before any catalog or level work runs.
    Commands,
    /// Builds or refreshes the [`LdtkMapCatalog`] from the loaded world JSON.
    Catalog,
    /// Captures entity and tile snapshots into runtime catalogs.
    Capture,
    /// Drives level-transition logic (load, activate, unload).
    LevelTransitions,
    /// Advances [`LdtkTileAnimator`] timers and swaps sprite indices.
    Animation,
}

/// Runtime configuration for the LDtk integration, inserted as a Bevy resource
/// before the plugin is added.
#[derive(Debug, Clone, Resource)]
pub struct LdtkConfig {
    /// Root directory that asset paths are resolved relative to (e.g. `"assets"`).
    pub asset_root: String,
    /// Asset-relative path to the `.ldtk` world file to load on startup, if any.
    pub world_asset_path: Option<String>,
    /// Whether external `.ldtkl` level files are discovered and cataloged.
    pub catalog_external_levels: bool,
    /// When `true`, levels adjacent to the active level are loaded proactively.
    pub load_level_neighbors: bool,
    /// IntGrid values that are treated as solid (impassable) by default.
    pub int_grid_solid_values: HashSet<i32>,
    /// Ordered list of collision rules that override the default solid-value behaviour.
    pub collision_rules: Vec<LdtkCollisionRule>,
    /// When non-empty, only layers whose identifiers appear here are processed.
    pub include_layers: HashSet<String>,
    /// Layers whose identifiers appear here are skipped even if in `include_layers`.
    pub exclude_layers: HashSet<String>,
    /// Runs structural validation after the world is cataloged when `true`.
    pub validate_on_load: bool,
    /// When `true`, validation warnings are promoted to errors that abort loading.
    pub strict_validation: bool,
    /// Emits a Bevy warning for every LDtk entity that has no registered spawner.
    pub warn_on_unregistered_entities: bool,
}

impl Default for LdtkConfig {
    fn default() -> Self {
        Self {
            asset_root: String::from("assets"),
            world_asset_path: None,
            catalog_external_levels: true,
            load_level_neighbors: true,
            int_grid_solid_values: HashSet::new(),
            collision_rules: Vec::new(),
            include_layers: HashSet::new(),
            exclude_layers: HashSet::new(),
            validate_on_load: true,
            strict_validation: false,
            warn_on_unregistered_entities: true,
        }
    }
}

impl LdtkConfig {
    /// Sets [`Self::world_asset_path`] and returns `self` for chaining.
    pub fn with_world_asset_path(mut self, path: impl Into<String>) -> Self {
        self.world_asset_path = Some(path.into());
        self
    }

    /// Overrides [`Self::asset_root`] and returns `self` for chaining.
    pub fn with_asset_root(mut self, path: impl Into<String>) -> Self {
        self.asset_root = path.into();
        self
    }

    /// Disables external-level cataloging and returns `self` for chaining.
    pub fn without_external_level_catalog(mut self) -> Self {
        self.catalog_external_levels = false;
        self
    }

    /// Replaces [`Self::int_grid_solid_values`] with `values` and returns `self` for chaining.
    pub fn with_solid_int_grid_values(mut self, values: impl IntoIterator<Item = i32>) -> Self {
        self.int_grid_solid_values = values.into_iter().collect();
        self
    }

    /// Replaces [`Self::collision_rules`] with `rules` and returns `self` for chaining.
    pub fn with_collision_rules(
        mut self,
        rules: impl IntoIterator<Item = LdtkCollisionRule>,
    ) -> Self {
        self.collision_rules = rules.into_iter().collect();
        self
    }

    /// Sets the layer allow-list and returns `self` for chaining.
    pub fn include_layers(mut self, layers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.include_layers = layers.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the layer deny-list and returns `self` for chaining.
    pub fn exclude_layers(mut self, layers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.exclude_layers = layers.into_iter().map(Into::into).collect();
        self
    }

    /// Disables on-load validation and returns `self` for chaining.
    pub fn without_validation(mut self) -> Self {
        self.validate_on_load = false;
        self
    }

    /// Suppresses warnings for unregistered LDtk entities and returns `self` for chaining.
    pub fn without_unregistered_entity_warnings(mut self) -> Self {
        self.warn_on_unregistered_entities = false;
        self
    }

    /// Enables strict validation (warnings become errors) and returns `self` for chaining.
    pub fn with_strict_validation(mut self) -> Self {
        self.strict_validation = true;
        self
    }

    /// Returns `true` when `layer_identifier` passes the include/exclude filter
    /// configured in [`Self::include_layers`] and [`Self::exclude_layers`].
    pub fn should_include_layer(&self, layer_identifier: &str) -> bool {
        (self.include_layers.is_empty() || self.include_layers.contains(layer_identifier))
            && !self.exclude_layers.contains(layer_identifier)
    }
}

/// Strategy for fetching the JSON of an external `.ldtkl` level file during
/// catalog construction. Injecting this keeps blocking filesystem I/O out of the
/// plugin core: the desktop default reads from disk, WASM gets nothing, and a
/// consumer can supply their own (e.g. an async-prefetched cache) by replacing
/// the [`LdtkExternalLevelSource`] resource.
pub trait ExternalLevelSource: Send + Sync + 'static {
    /// Returns the raw JSON text of the external level, or `None` if it cannot
    /// be provided. `world_path` is the asset-relative path of the `.ldtk` file,
    /// `rel_path` the level's `external_rel_path` relative to that file.
    fn load(&self, asset_root: &str, world_path: &str, rel_path: &str) -> Option<String>;
}

/// Resource holding the active external-level loader. `None` disables external
/// level cataloging (the default on targets without filesystem access).
#[derive(Resource, Default)]
pub struct LdtkExternalLevelSource(pub Option<Box<dyn ExternalLevelSource>>);

impl LdtkExternalLevelSource {
    /// Returns a reference to the inner [`ExternalLevelSource`], if one is set.
    pub fn source(&self) -> Option<&dyn ExternalLevelSource> {
        self.0.as_deref()
    }
}

/// Joins the asset root, the world file's directory and the level's relative
/// path into the on-disk location of an external `.ldtkl` file.
#[cfg(feature = "external-level-fs")]
pub fn external_level_path(
    asset_root: &str,
    active_world_path: &str,
    external_path: &str,
) -> std::path::PathBuf {
    let world_dir = std::path::Path::new(active_world_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new(""));
    std::path::Path::new(asset_root)
        .join(world_dir)
        .join(external_path)
}

/// Default [`ExternalLevelSource`] that reads external levels synchronously from
/// the filesystem. Only available with the `external-level-fs` feature.
#[cfg(feature = "external-level-fs")]
pub struct FsExternalLevelSource;

#[cfg(feature = "external-level-fs")]
impl ExternalLevelSource for FsExternalLevelSource {
    fn load(&self, asset_root: &str, world_path: &str, rel_path: &str) -> Option<String> {
        let full_path = external_level_path(asset_root, world_path, rel_path);
        std::fs::read_to_string(full_path).ok()
    }
}

/// A single rule that maps an IntGrid value (optionally scoped to a layer) to
/// collision behaviour, overriding the global [`LdtkConfig::int_grid_solid_values`] set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdtkCollisionRule {
    /// When `Some`, the rule only applies to the named layer; `None` matches every layer.
    pub layer_identifier: Option<String>,
    /// The IntGrid cell value this rule matches against.
    pub value: i32,
    /// When `true`, matching cells are treated as solid (impassable) colliders.
    pub solid: bool,
    /// When `true`, matching cells are treated as sensor (trigger) colliders.
    pub sensor: bool,
    /// Optional tag written to [`LdtkCollisionCell::tag`] for sensor cells.
    pub tag: Option<String>,
}

impl LdtkCollisionRule {
    /// Creates a rule that marks `value` as a solid collider on any layer.
    pub fn solid(value: i32) -> Self {
        Self {
            value,
            solid: true,
            ..Default::default()
        }
    }

    /// Creates a rule that marks `value` as a sensor collider with the given `tag` on any layer.
    pub fn sensor(value: i32, tag: impl Into<String>) -> Self {
        Self {
            value,
            sensor: true,
            tag: Some(tag.into()),
            ..Default::default()
        }
    }

    /// Scopes this rule to a single layer and returns `self` for chaining.
    pub fn for_layer(mut self, layer_identifier: impl Into<String>) -> Self {
        self.layer_identifier = Some(layer_identifier.into());
        self
    }
}

/// Bevy resource that tracks the current lifecycle state of the LDtk world.
#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkLoadState {
    /// Current phase of the load pipeline.
    pub status: LdtkLoadStatus,
    /// Identifier of the world that is loaded or being loaded, if known.
    pub world_identifier: Option<String>,
    /// Non-fatal warnings accumulated during the last load.
    pub warnings: Vec<String>,
    /// Fatal errors accumulated during the last load.
    pub errors: Vec<String>,
    /// Counters describing what was loaded in the last successful pass.
    pub stats: LdtkLoadStats,
}

impl LdtkLoadState {
    /// Returns `true` when the world has finished loading without errors.
    pub fn is_ready(&self) -> bool {
        self.status == LdtkLoadStatus::Ready
    }
}

/// Phase of the LDtk world load pipeline.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LdtkLoadStatus {
    /// No world has been requested yet.
    #[default]
    NotLoaded,
    /// A world load is in progress.
    Loading,
    /// The world loaded successfully and is ready to use.
    Ready,
    /// Loading failed; see [`LdtkLoadState::errors`] for details.
    Error,
}

/// Counters populated after a successful world load, useful for profiling and
/// validation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdtkLoadStats {
    /// Number of LDtk worlds cataloged.
    pub worlds: usize,
    /// Number of levels cataloged across all worlds.
    pub levels: usize,
    /// Number of layers cataloged across all levels.
    pub layers: usize,
    /// Number of tilesets cataloged.
    pub tilesets: usize,
    /// Total tile instances across all levels and layers.
    pub tiles: usize,
    /// Total entity instances across all levels.
    pub entities: usize,
    /// Number of spawn points extracted from entity layers.
    pub spawn_points: usize,
    /// Number of IntGrid cells with collision data.
    pub collision_cells: usize,
    /// Number of animated tiles found in tilesets.
    pub tile_animations: usize,
}

/// Bevy resource that accumulates validation issues found after loading; cleared
/// and repopulated on each reload.
#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkValidationReport {
    /// Non-fatal issues that do not abort loading.
    pub warnings: Vec<LdtkValidationIssue>,
    /// Fatal issues that abort loading when [`LdtkConfig::strict_validation`] is set.
    pub errors: Vec<LdtkValidationIssue>,
}

impl LdtkValidationReport {
    /// Removes all warnings and errors from the report.
    pub fn clear(&mut self) {
        self.warnings.clear();
        self.errors.clear();
    }

    /// Returns `true` if any errors have been recorded.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Records an issue, routing it to [`Self::errors`] when `strict` is set and
    /// to [`Self::warnings`] otherwise. This is the single place that encodes the
    /// "strict promotes warnings to errors" policy, so callers no longer repeat
    /// the branch at every check.
    pub fn push(&mut self, strict: bool, code: impl Into<String>, message: impl Into<String>) {
        let issue = LdtkValidationIssue::new(code, message);
        if strict {
            self.errors.push(issue);
        } else {
            self.warnings.push(issue);
        }
    }
}

/// A single validation issue with a short machine-readable `code` and a
/// human-readable `message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LdtkValidationIssue {
    /// Short identifier for the issue class (e.g. `"missing_tileset"`).
    pub code: String,
    /// Human-readable description of the specific problem.
    pub message: String,
}

impl LdtkValidationIssue {
    /// Severity is not stored on the issue itself; it is conveyed by which list
    /// of [`LdtkValidationReport`] the issue ends up in. Use
    /// [`LdtkValidationReport::push`] rather than constructing-and-placing by hand.
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Bevy resource that tracks what is currently active at runtime — which world
/// file is open, which level is active, and which levels are loaded.
#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkRuntimeState {
    /// Asset-relative path of the `.ldtk` file that is currently open.
    pub active_world_path: Option<String>,
    /// LDtk identifier of the active world.
    pub active_world_identifier: Option<String>,
    /// Bevy [`Entity`] that is the root of the spawned world hierarchy.
    pub active_world_root: Option<Entity>,
    /// LDtk identifier of the level currently focused by the camera / game logic.
    pub active_level: Option<String>,
    /// Current level-transition phase.
    pub transition: LdtkTransitionState,
    /// Identifiers of all levels that are currently spawned in the world.
    pub loaded_levels: HashSet<String>,
}

/// Phase of a level transition driven by the [`LdtkLoadSet::LevelTransitions`] systems.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LdtkTransitionState {
    /// No transition is in progress.
    #[default]
    Idle,
    /// A new level has been requested and is being loaded.
    Loading,
    /// The requested level has been loaded and activated.
    Active,
}

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
    /// Layers indexed by their LDtk `identifier`.
    pub layers: HashMap<String, LdtkLayerInfo>,
    /// Tilesets indexed by their numeric LDtk UID.
    pub tilesets: HashMap<i32, LdtkTilesetInfo>,
    /// Per-tile animation definitions indexed by [`LdtkTileKey`].
    pub tile_animations: HashMap<LdtkTileKey, LdtkTileAnimation>,
}

impl LdtkMapCatalog {
    /// Returns `true` when neither worlds nor levels have been cataloged yet.
    pub fn is_empty(&self) -> bool {
        self.worlds.is_empty() && self.levels.is_empty()
    }

    /// Inserts a level while keeping the `iid -> identifier` index in sync.
    pub fn insert_level_info(&mut self, info: LdtkLevelInfo) {
        self.levels_by_iid
            .insert(info.iid.clone(), info.identifier.clone());
        self.levels.insert(info.identifier.clone(), info);
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
}

/// Bevy resource containing all collision layers and cells extracted during
/// catalog construction.
#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkCollisionCatalog {
    /// Per-layer collision summaries, keyed by layer identifier.
    pub layers: HashMap<String, LdtkCollisionLayerInfo>,
    /// Flat list of every individual collision cell across all layers and levels.
    pub cells: Vec<LdtkCollisionCell>,
}

impl LdtkCollisionCatalog {
    /// World-space center of a collision cell, in the same Bevy y-up coordinate
    /// space as rendered tiles and [`LdtkSpawnPoint::position`] (Feature 4).
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

/// Fully qualified cross-reference to another LDtk entity instance.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdtkEntityReference {
    /// IID of the referenced entity instance.
    pub entity_iid: String,
    /// IID of the layer that contains the referenced entity.
    pub layer_iid: String,
    /// IID of the level that contains the referenced entity.
    pub level_iid: String,
    /// IID of the world that contains the referenced entity.
    pub world_iid: String,
}

/// A rectangular region inside a tileset, used by tile-typed LDtk field values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdtkTilesetRect {
    /// UID of the tileset this rectangle refers to.
    pub tileset_uid: i32,
    /// Top-left corner of the rectangle in pixels within the tileset image.
    pub position: IVec2,
    /// Width and height of the rectangle in pixels.
    pub size: IVec2,
}

/// Typed representation of any LDtk field value, covering all primitive and
/// composite types that LDtk supports.
#[derive(Debug, Clone, PartialEq)]
pub enum LdtkFieldValue {
    /// A boolean field value.
    Bool(bool),
    /// An integer field value stored as `i64` to accommodate all LDtk int ranges.
    Int(i64),
    /// A floating-point field value stored as `f64`.
    Float(f64),
    /// A string, file-path, or enum field value.
    String(String),
    /// A color field value represented as a Bevy [`Color`].
    Color(Color),
    /// A point field value; `None` when the field is set to null in the editor.
    Point(Option<IVec2>),
    /// A tile-reference field value; `None` when unset.
    Tile(Option<LdtkTilesetRect>),
    /// A cross-reference to another entity instance.
    EntityRef(LdtkEntityReference),
    /// An array field containing zero or more [`LdtkFieldValue`] elements.
    Array(Vec<LdtkFieldValue>),
    /// A null / unset field value.
    Null,
}

impl Default for LdtkFieldValue {
    fn default() -> Self {
        Self::Null
    }
}

impl LdtkFieldValue {
    /// Returns the inner `bool` if this is a [`Self::Bool`] variant, otherwise `None`.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the inner `i64` if this is a [`Self::Int`] variant, otherwise `None`.
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns the inner value as `f64`; accepts both [`Self::Float`] and [`Self::Int`].
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            Self::Int(value) => Some(*value as f64),
            _ => None,
        }
    }

    /// Returns a `&str` borrow if this is a [`Self::String`] variant, otherwise `None`.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns `Some(inner)` if this is a [`Self::Point`] variant (inner may itself be `None`),
    /// otherwise returns `None`.
    pub fn as_point(&self) -> Option<Option<IVec2>> {
        match self {
            Self::Point(value) => Some(*value),
            _ => None,
        }
    }

    /// Returns `Some(inner)` if this is a [`Self::Tile`] variant (inner may itself be `None`),
    /// otherwise returns `None`.
    pub fn as_tile(&self) -> Option<Option<&LdtkTilesetRect>> {
        match self {
            Self::Tile(value) => Some(value.as_ref()),
            _ => None,
        }
    }
}

/// Composite key that uniquely identifies a tile within a tileset, used to index
/// [`LdtkMapCatalog::tile_animations`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LdtkTileKey {
    /// UID of the owning tileset; `None` for embedded/internal tilesets.
    pub tileset_uid: Option<i32>,
    /// Zero-based index of the tile within the tileset.
    pub tile_id: i32,
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
    /// Column/row position of this tile in the layer grid (in grid units).
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

/// Frame sequence describing how a tile cycles through alternative tile IDs over
/// time. Attached to tile entities as a Bevy [`Component`].
#[derive(Debug, Clone, Component, Default)]
pub struct LdtkTileAnimation {
    /// Ordered list of frames; each frame specifies a tile ID and its display duration.
    pub frames: Vec<LdtkTileAnimationFrame>,
    /// When `true`, the animation loops back to frame 0 after the last frame; otherwise it holds.
    pub repeat: bool,
}

/// A single frame in an [`LdtkTileAnimation`], pairing a tile graphic with its
/// display duration.
#[derive(Debug, Clone, Default)]
pub struct LdtkTileAnimationFrame {
    /// Index of the tile in the tileset to display for this frame.
    pub tile_id: i32,
    /// How long this frame is shown, in seconds.
    pub duration: f32,
}

/// Bevy [`Component`] that drives frame advancement for an animated tile entity,
/// holding the animation data, current frame index, and an internal [`Timer`].
#[derive(Debug, Clone, Component)]
pub struct LdtkTileAnimator {
    /// The animation definition being played.
    pub animation: LdtkTileAnimation,
    /// Zero-based index of the frame that is currently displayed.
    pub frame_index: usize,
    /// Countdown timer set to the current frame's duration.
    pub timer: Timer,
}

impl LdtkTileAnimator {
    /// Creates a new animator for `animation`, initialising the timer to the
    /// duration of the first frame (clamped to a minimum of 0.001 s).
    pub fn new(animation: LdtkTileAnimation) -> Self {
        let duration = animation
            .frames
            .first()
            .map(|frame| frame.duration)
            .unwrap_or(0.1)
            .max(0.001);

        Self {
            animation,
            frame_index: 0,
            timer: Timer::from_seconds(duration, TimerMode::Repeating),
        }
    }

    /// Advances the animation by `delta`. Returns the tile id of the new frame
    /// when the frame changed this tick, otherwise `None`. This is the single
    /// source of truth for frame stepping shared by every animator system.
    pub fn advance(&mut self, delta: Duration) -> Option<i32> {
        self.timer.tick(delta);
        if !self.timer.just_finished() || self.animation.frames.is_empty() {
            return None;
        }

        self.frame_index += 1;
        if self.frame_index >= self.animation.frames.len() {
            self.frame_index = if self.animation.repeat {
                0
            } else {
                self.animation.frames.len() - 1
            };
        }

        let duration = self.animation.frames[self.frame_index].duration.max(0.001);
        self.timer = Timer::from_seconds(duration, TimerMode::Repeating);
        self.animation
            .frames
            .get(self.frame_index)
            .map(|frame| frame.tile_id)
    }
}

/// All data needed to spawn a single LDtk entity instance, passed to registered
/// spawner callbacks.
#[derive(Debug, Clone, Default)]
pub struct LdtkEntitySpawnContext {
    /// Instance-unique IID for this entity.
    pub entity_iid: String,
    /// LDtk identifier of the entity definition (e.g. `"Enemy"`).
    pub entity_identifier: String,
    /// Identifier of the world this entity belongs to, if known.
    pub world_identifier: Option<String>,
    /// Identifier of the level this entity lives in, if known.
    pub level_identifier: Option<String>,
    /// Identifier of the layer this entity lives on, if known.
    pub layer_identifier: Option<String>,
    /// World-space position of the entity's pivot point in pixels.
    pub position: Vec2,
    /// Column/row position of the entity in the layer grid (in grid units).
    pub grid_position: IVec2,
    /// Pixel dimensions of this entity instance.
    pub size: Vec2,
    /// Normalised pivot point, where `(0,0)` is top-left and `(1,1)` is bottom-right.
    pub pivot: Vec2,
    /// Tags defined on this entity instance in the LDtk editor.
    pub tags: Vec<String>,
    /// Optional visual tile assigned to this entity in the LDtk editor.
    pub tile: Option<LdtkTileMetadata>,
    /// Custom field values defined on this entity instance.
    pub field_values: HashMap<String, LdtkFieldValue>,
}

/// Shared typed accessors for anything that carries LDtk field instances.
/// Implemented by both the live snapshot ([`LdtkImportedEntity`]) and the
/// spawn-time context ([`LdtkEntitySpawnContext`]) so the lookup logic lives
/// in exactly one place.
pub trait LdtkFieldAccess {
    /// Returns a reference to the underlying field-value map.
    fn field_values(&self) -> &HashMap<String, LdtkFieldValue>;

    /// Looks up a field by `identifier`, returning `None` when absent.
    fn field(&self, identifier: &str) -> Option<&LdtkFieldValue> {
        self.field_values().get(identifier)
    }

    /// Returns the boolean value of `identifier`, or `None` if absent or of a different type.
    fn field_bool(&self, identifier: &str) -> Option<bool> {
        self.field(identifier).and_then(LdtkFieldValue::as_bool)
    }

    /// Returns the integer value of `identifier` as `i64`, or `None` if absent or of a different type.
    fn field_i64(&self, identifier: &str) -> Option<i64> {
        self.field(identifier).and_then(LdtkFieldValue::as_i64)
    }

    /// Returns the numeric value of `identifier` as `f64` (accepts int and float fields),
    /// or `None` if absent or incompatible.
    fn field_f64(&self, identifier: &str) -> Option<f64> {
        self.field(identifier).and_then(LdtkFieldValue::as_f64)
    }

    /// Returns a `&str` borrow of `identifier`'s value, or `None` if absent or non-string.
    fn field_str(&self, identifier: &str) -> Option<&str> {
        self.field(identifier).and_then(LdtkFieldValue::as_str)
    }
}

impl LdtkFieldAccess for LdtkEntitySpawnContext {
    fn field_values(&self) -> &HashMap<String, LdtkFieldValue> {
        &self.field_values
    }
}

/// Snapshot of an LDtk entity instance stored in [`LdtkEntityCatalog`] and also
/// attached as a Bevy [`Component`] to the spawned entity.
#[derive(Debug, Clone, Component, Default)]
pub struct LdtkImportedEntity {
    /// Instance-unique IID for this entity.
    pub entity_iid: String,
    /// LDtk identifier of the entity definition (e.g. `"Chest"`).
    pub entity_identifier: String,
    /// Identifier of the world this entity belongs to, if known.
    pub world_identifier: Option<String>,
    /// Identifier of the level this entity lives in, if known.
    pub level_identifier: Option<String>,
    /// Identifier of the layer this entity lives on, if known.
    pub layer_identifier: Option<String>,
    /// World-space position of the entity's pivot point in pixels.
    pub position: Vec2,
    /// Column/row position of the entity in the layer grid (in grid units).
    pub grid_position: IVec2,
    /// Pixel dimensions of this entity instance.
    pub size: Vec2,
    /// Normalised pivot point, where `(0,0)` is top-left and `(1,1)` is bottom-right.
    pub pivot: Vec2,
    /// Tags defined on this entity instance in the LDtk editor.
    pub tags: Vec<String>,
    /// Optional visual tile assigned to this entity in the LDtk editor.
    pub tile: Option<LdtkTileMetadata>,
    /// Custom field values defined on this entity instance.
    pub field_values: HashMap<String, LdtkFieldValue>,
}

impl LdtkFieldAccess for LdtkImportedEntity {
    fn field_values(&self) -> &HashMap<String, LdtkFieldValue> {
        &self.field_values
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

/// Bevy [`Component`] that marks an entity as originating from an LDtk entity
/// instance, carrying its definition name and location identifiers.
#[derive(Debug, Clone, Component, Default)]
pub struct LdtkEntityMarker {
    /// LDtk identifier of the entity definition (e.g. `"Boss"`).
    pub definition_identifier: String,
    /// Identifier of the level the entity was spawned from, if known.
    pub level_identifier: Option<String>,
    /// Identifier of the world the entity was spawned from, if known.
    pub world_identifier: Option<String>,
}

/// Bevy [`Component`] marker placed on the root entity of a spawned LDtk world.
#[derive(Debug, Clone, Component, Default)]
pub struct LdtkWorldRoot;

/// Bevy [`Component`] marker that prevents an entity from being despawned during
/// level transitions.
#[derive(Debug, Clone, Component, Default)]
pub struct LdtkPersistent;

/// Bevy [`Component`] that records the collision role of a spawned tile or
/// entity.
#[derive(Debug, Clone, Component, Default)]
pub struct LdtkCollider {
    /// Whether this collider blocks movement.
    pub solid: bool,
    /// Whether this collider is a sensor (trigger-only, non-blocking).
    pub sensor: bool,
}

/// Bevy [`Component`] attached to tile entities that carry collision data,
/// linking them back to their source level and IntGrid value.
#[derive(Debug, Clone, Component, Default)]
pub struct LdtkTileCollision {
    /// Identifier of the level that owns this tile.
    pub level_identifier: String,
    /// Index of the tile in its tileset.
    pub tile_id: i32,
    /// Whether the tile is a solid (impassable) collider.
    pub solid: bool,
}

/// Commands that can be submitted to the [`LdtkCommandQueue`] to drive the LDtk
/// runtime.
#[derive(Debug, Clone)]
pub enum LdtkCommand {
    /// Load and spawn the world at `world_path`.
    SpawnWorld { world_path: String },
    /// Transition to the level identified by `level_identifier`.
    ChangeLevel { level_identifier: String },
    /// Reload the currently active world from disk.
    ReloadWorld,
    /// Despawn the active world and reset runtime state.
    UnloadWorld,
}

/// Bevy resource acting as a deferred queue for [`LdtkCommand`]s, which are
/// processed at the start of each frame by the [`LdtkLoadSet::Commands`] systems.
#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkCommandQueue {
    /// Commands waiting to be processed this frame.
    pub pending: Vec<LdtkCommand>,
}

/// Bevy event fired to request that a world file is loaded and spawned.
#[derive(Debug, Clone, Message)]
pub struct LdtkSpawnWorldEvent {
    /// Asset-relative path to the `.ldtk` file to load.
    pub world_path: String,
}

/// Bevy event fired once after the [`LdtkMapCatalog`] has been fully populated
/// for a world.
#[derive(Debug, Clone, Message)]
pub struct LdtkMapLoadedEvent {
    /// LDtk identifier of the world that was loaded.
    pub world_identifier: String,
}

/// Bevy event fired when a level becomes the active level after a transition.
#[derive(Debug, Clone, Message)]
pub struct LdtkLevelActivatedEvent {
    /// LDtk identifier of the level that was activated.
    pub level_identifier: String,
}

/// Bevy event fired after the active world has been fully despawned.
#[derive(Debug, Clone, Message)]
pub struct LdtkWorldUnloadedEvent;

/// Bevy event fired after an on-load validation pass completes, summarising the
/// number of issues found.
#[derive(Debug, Clone, Message)]
pub struct LdtkValidationFinishedEvent {
    /// Number of validation warnings produced.
    pub warnings: usize,
    /// Number of validation errors produced.
    pub errors: usize,
}

/// Composite key used to look up a registered spawner in [`LdtkEntityRegistry`],
/// supporting both exact (layer + entity) and wildcard (entity-only or default)
/// matches.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LdtkEntityRegistryKey {
    /// When `Some`, the spawner only applies to this layer; `None` matches any layer.
    pub layer_identifier: Option<String>,
    /// When `Some`, the spawner only applies to this entity definition; `None` is the catch-all default.
    pub entity_identifier: Option<String>,
}

/// Type alias for a boxed spawner callback stored in [`LdtkEntityRegistry`].
pub type LdtkEntitySpawner =
    Box<dyn Fn(&mut World, Entity, &LdtkEntitySpawnContext) + Send + Sync + 'static>;

/// Bevy resource that maps LDtk entity definitions to Bevy spawner callbacks,
/// resolved at runtime when entity instances are encountered.
#[derive(Resource, Default)]
pub struct LdtkEntityRegistry {
    /// Registered spawners keyed by [`LdtkEntityRegistryKey`].
    pub spawners: HashMap<LdtkEntityRegistryKey, LdtkEntitySpawner>,
}

impl LdtkEntityRegistry {
    /// Registers `B::default()` as the bundle to insert for any entity matching
    /// `identifier`, regardless of which layer it is on.
    pub fn register_bundle<B>(&mut self, identifier: impl Into<String>)
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.register_bundle_for_layer_optional::<B>(None, Some(identifier.into()));
    }

    /// Registers `B::default()` as the bundle to insert for entities matching
    /// both `layer_identifier` and `identifier`.
    pub fn register_bundle_for_layer<B>(
        &mut self,
        layer_identifier: impl Into<String>,
        identifier: impl Into<String>,
    ) where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.register_bundle_for_layer_optional::<B>(
            Some(layer_identifier.into()),
            Some(identifier.into()),
        );
    }

    /// Registers `B::default()` as the fallback bundle for any unmatched entity
    /// on `layer_identifier`.
    pub fn register_default_bundle_for_layer<B>(&mut self, layer_identifier: impl Into<String>)
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.register_bundle_for_layer_optional::<B>(Some(layer_identifier.into()), None);
    }

    /// Registers `B::default()` as the global fallback bundle for any entity not
    /// matched by a more specific registration.
    pub fn register_default_bundle<B>(&mut self)
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.register_bundle_for_layer_optional::<B>(None, None);
    }

    /// Low-level registration that accepts optional layer and entity identifiers
    /// directly; prefer the typed helpers above for clarity.
    pub fn register_bundle_for_layer_optional<B>(
        &mut self,
        layer_identifier: Option<String>,
        entity_identifier: Option<String>,
    ) where
        B: Bundle + Default + Send + Sync + 'static,
    {
        let key = LdtkEntityRegistryKey {
            layer_identifier,
            entity_identifier,
        };

        self.spawners.insert(
            key,
            Box::new(
                move |world: &mut World, entity: Entity, context: &LdtkEntitySpawnContext| {
                    world.entity_mut(entity).insert((
                        B::default(),
                        LdtkEntityMarker {
                            definition_identifier: context.entity_identifier.clone(),
                            level_identifier: context.level_identifier.clone(),
                            world_identifier: context.world_identifier.clone(),
                        },
                        Transform::from_translation(context.position.extend(0.0)),
                        GlobalTransform::default(),
                    ));
                },
            ),
        );
    }

    /// Registers a custom `spawner` closure for any entity matching `identifier`,
    /// regardless of layer.
    pub fn register_spawner(
        &mut self,
        identifier: impl Into<String>,
        spawner: impl Fn(&mut World, Entity, &LdtkEntitySpawnContext) + Send + Sync + 'static,
    ) {
        self.register_spawner_for_layer_optional(None, Some(identifier.into()), spawner);
    }

    /// Registers a custom `spawner` closure for entities matching both
    /// `layer_identifier` and `entity_identifier`.
    pub fn register_spawner_for_layer(
        &mut self,
        layer_identifier: impl Into<String>,
        entity_identifier: impl Into<String>,
        spawner: impl Fn(&mut World, Entity, &LdtkEntitySpawnContext) + Send + Sync + 'static,
    ) {
        self.register_spawner_for_layer_optional(
            Some(layer_identifier.into()),
            Some(entity_identifier.into()),
            spawner,
        );
    }

    /// Low-level spawner registration accepting optional identifiers directly;
    /// prefer the typed helpers above for clarity.
    pub fn register_spawner_for_layer_optional(
        &mut self,
        layer_identifier: Option<String>,
        entity_identifier: Option<String>,
        spawner: impl Fn(&mut World, Entity, &LdtkEntitySpawnContext) + Send + Sync + 'static,
    ) {
        self.spawners.insert(
            LdtkEntityRegistryKey {
                layer_identifier,
                entity_identifier,
            },
            Box::new(spawner),
        );
    }

    /// Resolves the best matching spawner for an entity instance using a four-level
    /// priority: exact (layer + entity) > entity-only > layer-only > global default.
    /// Returns `None` when no spawner has been registered for this combination.
    pub fn resolve(
        &self,
        layer_identifier: Option<&str>,
        entity_identifier: &str,
    ) -> Option<&LdtkEntitySpawner> {
        let exact = LdtkEntityRegistryKey {
            layer_identifier: layer_identifier.map(ToOwned::to_owned),
            entity_identifier: Some(entity_identifier.to_string()),
        };
        let entity_only = LdtkEntityRegistryKey {
            layer_identifier: None,
            entity_identifier: Some(entity_identifier.to_string()),
        };
        let layer_only = layer_identifier.map(|layer| LdtkEntityRegistryKey {
            layer_identifier: Some(layer.to_string()),
            entity_identifier: None,
        });
        let default = LdtkEntityRegistryKey {
            layer_identifier: None,
            entity_identifier: None,
        };

        self.spawners
            .get(&exact)
            .or_else(|| self.spawners.get(&entity_only))
            .or_else(|| layer_only.as_ref().and_then(|key| self.spawners.get(key)))
            .or_else(|| self.spawners.get(&default))
    }
}

impl From<&bevy_ecs_ldtk::ldtk::ReferenceToAnEntityInstance> for LdtkEntityReference {
    fn from(value: &bevy_ecs_ldtk::ldtk::ReferenceToAnEntityInstance) -> Self {
        Self {
            entity_iid: value.entity_iid.clone(),
            layer_iid: value.layer_iid.clone(),
            level_iid: value.level_iid.clone(),
            world_iid: value.world_iid.clone(),
        }
    }
}

impl From<&bevy_ecs_ldtk::ldtk::TilesetRectangle> for LdtkTilesetRect {
    fn from(value: &bevy_ecs_ldtk::ldtk::TilesetRectangle) -> Self {
        Self {
            tileset_uid: value.tileset_uid,
            position: IVec2::new(value.x, value.y),
            size: IVec2::new(value.w, value.h),
        }
    }
}

impl From<&bevy_ecs_ldtk::ldtk::FieldInstance> for LdtkFieldValue {
    fn from(value: &bevy_ecs_ldtk::ldtk::FieldInstance) -> Self {
        use bevy_ecs_ldtk::ldtk::FieldValue;

        match &value.value {
            FieldValue::Int(v) => Self::Int(i64::from(v.unwrap_or_default())),
            FieldValue::Float(v) => Self::Float(f64::from(v.unwrap_or_default())),
            FieldValue::Bool(v) => Self::Bool(*v),
            FieldValue::String(v) => Self::String(v.clone().unwrap_or_default()),
            FieldValue::Color(v) => Self::Color(*v),
            FieldValue::FilePath(v) => Self::String(v.clone().unwrap_or_default()),
            FieldValue::Enum(v) => Self::String(v.clone().unwrap_or_default()),
            FieldValue::Tile(v) => Self::Tile(v.as_ref().map(LdtkTilesetRect::from)),
            FieldValue::EntityRef(v) => Self::EntityRef(
                v.as_ref()
                    .map(LdtkEntityReference::from)
                    .unwrap_or_default(),
            ),
            FieldValue::Point(v) => Self::Point(*v),
            FieldValue::Ints(v) => Self::Array(
                v.iter()
                    .map(|entry| entry.map(|i| Self::Int(i64::from(i))).unwrap_or(Self::Null))
                    .collect(),
            ),
            FieldValue::Floats(v) => Self::Array(
                v.iter()
                    .map(|entry| {
                        entry
                            .map(|f| Self::Float(f64::from(f)))
                            .unwrap_or(Self::Null)
                    })
                    .collect(),
            ),
            FieldValue::Bools(v) => Self::Array(v.iter().map(|entry| Self::Bool(*entry)).collect()),
            FieldValue::Strings(v) | FieldValue::FilePaths(v) | FieldValue::Enums(v) => {
                Self::Array(
                    v.iter()
                        .map(|entry| {
                            entry
                                .as_ref()
                                .map(|text| Self::String(text.clone()))
                                .unwrap_or(Self::Null)
                        })
                        .collect(),
                )
            }
            FieldValue::Colors(v) => {
                Self::Array(v.iter().map(|entry| Self::Color(*entry)).collect())
            }
            FieldValue::Tiles(v) => Self::Array(
                v.iter()
                    .map(|entry| Self::Tile(entry.as_ref().map(LdtkTilesetRect::from)))
                    .collect(),
            ),
            FieldValue::EntityRefs(v) => Self::Array(
                v.iter()
                    .map(|entry| {
                        entry
                            .as_ref()
                            .map(|reference| Self::EntityRef(LdtkEntityReference::from(reference)))
                            .unwrap_or(Self::Null)
                    })
                    .collect(),
            ),
            FieldValue::Points(v) => {
                Self::Array(v.iter().map(|entry| Self::Point(*entry)).collect())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_helpers_return_typed_values() {
        assert_eq!(LdtkFieldValue::Bool(true).as_bool(), Some(true));
        assert_eq!(LdtkFieldValue::Int(42).as_i64(), Some(42));
        assert_eq!(LdtkFieldValue::Int(42).as_f64(), Some(42.0));
        assert_eq!(LdtkFieldValue::Float(1.5).as_f64(), Some(1.5));
        assert_eq!(
            LdtkFieldValue::String("door_a".to_string()).as_str(),
            Some("door_a")
        );
        assert_eq!(LdtkFieldValue::Null.as_bool(), None);
    }

    #[test]
    fn entity_field_helpers_read_from_snapshot() {
        let mut entity = LdtkImportedEntity::default();
        entity
            .field_values
            .insert("damage".to_string(), LdtkFieldValue::Int(7));
        entity
            .field_values
            .insert("locked".to_string(), LdtkFieldValue::Bool(false));

        assert_eq!(entity.field_i64("damage"), Some(7));
        assert_eq!(entity.field_bool("locked"), Some(false));
        assert_eq!(entity.field_str("missing"), None);
    }

    #[test]
    fn config_layer_filters_are_combined() {
        let config = LdtkConfig::default()
            .include_layers(["Ground", "Entities"])
            .exclude_layers(["Debug"]);

        assert!(config.should_include_layer("Ground"));
        assert!(!config.should_include_layer("Background"));
        assert!(!config.should_include_layer("Debug"));
    }

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

    #[cfg(feature = "external-level-fs")]
    #[test]
    fn builds_external_level_path_relative_to_world_file() {
        let path = external_level_path(
            "assets",
            "worlds/SeparateLevelFiles.ldtk",
            "SeparateLevelFiles/World_Level_0.ldtkl",
        );

        assert_eq!(
            path,
            std::path::PathBuf::from("assets")
                .join("worlds")
                .join("SeparateLevelFiles")
                .join("World_Level_0.ldtkl")
        );
    }
}
