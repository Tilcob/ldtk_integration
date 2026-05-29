use bevy::prelude::*;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

/// Explicit ordering for the LDtk systems so dependent stages run in a
/// deterministic sequence instead of relying on tuple insertion order.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum LdtkLoadSet {
    Commands,
    Catalog,
    Capture,
    LevelTransitions,
    Animation,
}

#[derive(Debug, Clone, Resource)]
pub struct LdtkConfig {
    pub asset_root: String,
    pub world_asset_path: Option<String>,
    pub catalog_external_levels: bool,
    pub load_level_neighbors: bool,
    pub int_grid_solid_values: HashSet<i32>,
    pub collision_rules: Vec<LdtkCollisionRule>,
    pub include_layers: HashSet<String>,
    pub exclude_layers: HashSet<String>,
    pub validate_on_load: bool,
    pub strict_validation: bool,
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
    pub fn with_world_asset_path(mut self, path: impl Into<String>) -> Self {
        self.world_asset_path = Some(path.into());
        self
    }

    pub fn with_asset_root(mut self, path: impl Into<String>) -> Self {
        self.asset_root = path.into();
        self
    }

    pub fn without_external_level_catalog(mut self) -> Self {
        self.catalog_external_levels = false;
        self
    }

    pub fn with_solid_int_grid_values(mut self, values: impl IntoIterator<Item = i32>) -> Self {
        self.int_grid_solid_values = values.into_iter().collect();
        self
    }

    pub fn with_collision_rules(
        mut self,
        rules: impl IntoIterator<Item = LdtkCollisionRule>,
    ) -> Self {
        self.collision_rules = rules.into_iter().collect();
        self
    }

    pub fn include_layers(mut self, layers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.include_layers = layers.into_iter().map(Into::into).collect();
        self
    }

    pub fn exclude_layers(mut self, layers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.exclude_layers = layers.into_iter().map(Into::into).collect();
        self
    }

    pub fn without_validation(mut self) -> Self {
        self.validate_on_load = false;
        self
    }

    pub fn without_unregistered_entity_warnings(mut self) -> Self {
        self.warn_on_unregistered_entities = false;
        self
    }

    pub fn with_strict_validation(mut self) -> Self {
        self.strict_validation = true;
        self
    }

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdtkCollisionRule {
    pub layer_identifier: Option<String>,
    pub value: i32,
    pub solid: bool,
    pub sensor: bool,
    pub tag: Option<String>,
}

impl LdtkCollisionRule {
    pub fn solid(value: i32) -> Self {
        Self {
            value,
            solid: true,
            ..Default::default()
        }
    }

    pub fn sensor(value: i32, tag: impl Into<String>) -> Self {
        Self {
            value,
            sensor: true,
            tag: Some(tag.into()),
            ..Default::default()
        }
    }

    pub fn for_layer(mut self, layer_identifier: impl Into<String>) -> Self {
        self.layer_identifier = Some(layer_identifier.into());
        self
    }
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkLoadState {
    pub status: LdtkLoadStatus,
    pub world_identifier: Option<String>,
    pub warnings: Vec<String>,
    pub errors: Vec<String>,
    pub stats: LdtkLoadStats,
}

impl LdtkLoadState {
    pub fn is_ready(&self) -> bool {
        self.status == LdtkLoadStatus::Ready
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LdtkLoadStatus {
    #[default]
    NotLoaded,
    Loading,
    Ready,
    Error,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdtkLoadStats {
    pub worlds: usize,
    pub levels: usize,
    pub layers: usize,
    pub tilesets: usize,
    pub tiles: usize,
    pub entities: usize,
    pub spawn_points: usize,
    pub collision_cells: usize,
    pub tile_animations: usize,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkValidationReport {
    pub warnings: Vec<LdtkValidationIssue>,
    pub errors: Vec<LdtkValidationIssue>,
}

impl LdtkValidationReport {
    pub fn clear(&mut self) {
        self.warnings.clear();
        self.errors.clear();
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LdtkValidationIssue {
    pub code: String,
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

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkRuntimeState {
    pub active_world_path: Option<String>,
    pub active_world_identifier: Option<String>,
    pub active_world_root: Option<Entity>,
    pub active_level: Option<String>,
    pub transition: LdtkTransitionState,
    pub loaded_levels: HashSet<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LdtkTransitionState {
    #[default]
    Idle,
    Loading,
    Active,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkMapCatalog {
    pub worlds: HashMap<String, LdtkWorldInfo>,
    /// Levels keyed by their LDtk `identifier`.
    pub levels: HashMap<String, LdtkLevelInfo>,
    /// Secondary index mapping a level `iid` to its `identifier`, so lookups by
    /// iid are O(1) instead of a linear scan over [`Self::levels`]. Kept in sync
    /// by [`Self::insert_level_info`].
    pub levels_by_iid: HashMap<String, String>,
    pub layers: HashMap<String, LdtkLayerInfo>,
    pub tilesets: HashMap<i32, LdtkTilesetInfo>,
    pub tile_animations: HashMap<LdtkTileKey, LdtkTileAnimation>,
}

impl LdtkMapCatalog {
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

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkCollisionCatalog {
    pub layers: HashMap<String, LdtkCollisionLayerInfo>,
    pub cells: Vec<LdtkCollisionCell>,
}

/// Strongly typed mirror of `bevy_ecs_ldtk`'s layer kind, so consumers match on
/// a stable enum instead of a `format!("{:?}", ...)` debug string that silently
/// breaks if the upstream `Debug` impl changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LdtkLayerType {
    IntGrid,
    Entities,
    Tiles,
    AutoLayer,
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

#[derive(Debug, Clone, Default)]
pub struct LdtkCollisionLayerInfo {
    pub level_identifier: String,
    pub level_iid: String,
    pub layer_identifier: String,
    pub layer_iid: String,
    pub layer_type: LdtkLayerType,
    pub solid_cells: usize,
    pub tile_cells: usize,
    pub sensor_cells: usize,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkCollisionCell {
    pub level_identifier: String,
    pub level_iid: String,
    pub layer_identifier: String,
    pub layer_iid: String,
    pub grid_position: IVec2,
    pub value: i32,
    pub solid: bool,
    pub sensor: bool,
    pub tag: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkWorldInfo {
    pub identifier: String,
    pub path: String,
    pub levels: Vec<String>,
    pub layout: LdtkWorldLayout,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkLevelInfo {
    pub iid: String,
    pub identifier: String,
    pub world_identifier: String,
    pub external_path: Option<String>,
    pub size: IVec2,
    pub world_position: IVec2,
    pub neighbors: Vec<LdtkNeighbor>,
    pub spawn_points: Vec<LdtkSpawnPoint>,
    pub tiles: Vec<LdtkTileMetadata>,
    pub entities: Vec<LdtkImportedEntity>,
    pub fields: HashMap<String, LdtkFieldValue>,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkLayerInfo {
    pub iid: String,
    pub identifier: String,
    pub level_identifier: String,
    pub layer_type: LdtkLayerType,
    pub grid_size: i32,
    pub grid_size_cells: IVec2,
    pub tileset_uid: Option<i32>,
    pub tileset_rel_path: Option<String>,
    pub opacity: f32,
    pub visible: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkTilesetInfo {
    pub uid: i32,
    pub identifier: String,
    pub rel_path: Option<String>,
    pub tile_grid_size: i32,
    pub grid_size_cells: IVec2,
    pub image_size: IVec2,
    pub spacing: i32,
    pub padding: i32,
    pub tags: Vec<String>,
    pub tile_tags: HashMap<i32, Vec<String>>,
    pub custom_data: HashMap<i32, String>,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkNeighbor {
    pub level_identifier: String,
    pub direction: LdtkDirection,
    pub cost: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum LdtkDirection {
    #[default]
    North,
    South,
    East,
    West,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LdtkWorldLayout {
    #[default]
    Free,
    GridVania,
    LinearHorizontal,
    LinearVertical,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkSpawnPoint {
    pub identifier: String,
    pub position: Vec2,
    pub level_identifier: String,
    pub layer_identifier: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdtkEntityReference {
    pub entity_iid: String,
    pub layer_iid: String,
    pub level_iid: String,
    pub world_iid: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdtkTilesetRect {
    pub tileset_uid: i32,
    pub position: IVec2,
    pub size: IVec2,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LdtkFieldValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Color(Color),
    Point(Option<IVec2>),
    Tile(Option<LdtkTilesetRect>),
    EntityRef(LdtkEntityReference),
    Array(Vec<LdtkFieldValue>),
    Null,
}

impl Default for LdtkFieldValue {
    fn default() -> Self {
        Self::Null
    }
}

impl LdtkFieldValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Float(value) => Some(*value),
            Self::Int(value) => Some(*value as f64),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_point(&self) -> Option<Option<IVec2>> {
        match self {
            Self::Point(value) => Some(*value),
            _ => None,
        }
    }

    pub fn as_tile(&self) -> Option<Option<&LdtkTilesetRect>> {
        match self {
            Self::Tile(value) => Some(value.as_ref()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LdtkTileKey {
    pub tileset_uid: Option<i32>,
    pub tile_id: i32,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkTileMetadata {
    pub level_identifier: String,
    pub layer_identifier: String,
    pub layer_iid: String,
    pub tileset_uid: Option<i32>,
    pub tileset_identifier: String,
    pub tile_id: i32,
    pub layer_position: IVec2,
    pub source_position: IVec2,
    // LDtk tiles carry no native rotation, only flip flags. A 180° rotation is
    // simply `flip_x && flip_y`; consumers derive that themselves instead of us
    // shipping a redundant (and previously incorrect) `rotation_degrees` field.
    pub flip_x: bool,
    pub flip_y: bool,
    pub alpha: f32,
    pub custom_data: Option<String>,
    pub tags: Vec<String>,
    pub animation: Option<LdtkTileAnimation>,
}

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkTileAnimation {
    pub frames: Vec<LdtkTileAnimationFrame>,
    pub repeat: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkTileAnimationFrame {
    pub tile_id: i32,
    pub duration: f32,
}

#[derive(Debug, Clone, Component)]
pub struct LdtkTileAnimator {
    pub animation: LdtkTileAnimation,
    pub frame_index: usize,
    pub timer: Timer,
}

impl LdtkTileAnimator {
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

#[derive(Debug, Clone, Default)]
pub struct LdtkEntitySpawnContext {
    pub entity_iid: String,
    pub entity_identifier: String,
    pub world_identifier: Option<String>,
    pub level_identifier: Option<String>,
    pub layer_identifier: Option<String>,
    pub position: Vec2,
    pub grid_position: IVec2,
    pub size: Vec2,
    pub pivot: Vec2,
    pub tags: Vec<String>,
    pub tile: Option<LdtkTileMetadata>,
    pub field_values: HashMap<String, LdtkFieldValue>,
}

/// Shared typed accessors for anything that carries LDtk field instances.
/// Implemented by both the live snapshot ([`LdtkImportedEntity`]) and the
/// spawn-time context ([`LdtkEntitySpawnContext`]) so the lookup logic lives
/// in exactly one place.
pub trait LdtkFieldAccess {
    fn field_values(&self) -> &HashMap<String, LdtkFieldValue>;

    fn field(&self, identifier: &str) -> Option<&LdtkFieldValue> {
        self.field_values().get(identifier)
    }

    fn field_bool(&self, identifier: &str) -> Option<bool> {
        self.field(identifier).and_then(LdtkFieldValue::as_bool)
    }

    fn field_i64(&self, identifier: &str) -> Option<i64> {
        self.field(identifier).and_then(LdtkFieldValue::as_i64)
    }

    fn field_f64(&self, identifier: &str) -> Option<f64> {
        self.field(identifier).and_then(LdtkFieldValue::as_f64)
    }

    fn field_str(&self, identifier: &str) -> Option<&str> {
        self.field(identifier).and_then(LdtkFieldValue::as_str)
    }
}

impl LdtkFieldAccess for LdtkEntitySpawnContext {
    fn field_values(&self) -> &HashMap<String, LdtkFieldValue> {
        &self.field_values
    }
}

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkImportedEntity {
    pub entity_iid: String,
    pub entity_identifier: String,
    pub world_identifier: Option<String>,
    pub level_identifier: Option<String>,
    pub layer_identifier: Option<String>,
    pub position: Vec2,
    pub grid_position: IVec2,
    pub size: Vec2,
    pub pivot: Vec2,
    pub tags: Vec<String>,
    pub tile: Option<LdtkTileMetadata>,
    pub field_values: HashMap<String, LdtkFieldValue>,
}

impl LdtkFieldAccess for LdtkImportedEntity {
    fn field_values(&self) -> &HashMap<String, LdtkFieldValue> {
        &self.field_values
    }
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkEntityCatalog {
    pub by_iid: HashMap<String, Entity>,
    pub snapshots: HashMap<String, LdtkImportedEntity>,
}

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkEntityMarker {
    pub definition_identifier: String,
    pub level_identifier: Option<String>,
    pub world_identifier: Option<String>,
}

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkWorldRoot;

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkPersistent;

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkCollider {
    pub solid: bool,
    pub sensor: bool,
}

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkTileCollision {
    pub level_identifier: String,
    pub tile_id: i32,
    pub solid: bool,
}

#[derive(Debug, Clone)]
pub enum LdtkCommand {
    SpawnWorld { world_path: String },
    ChangeLevel { level_identifier: String },
    ReloadWorld,
    UnloadWorld,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkCommandQueue {
    pub pending: Vec<LdtkCommand>,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkSpawnWorldEvent {
    pub world_path: String,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkMapLoadedEvent {
    pub world_identifier: String,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkLevelActivatedEvent {
    pub level_identifier: String,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkWorldUnloadedEvent;

#[derive(Debug, Clone, Message)]
pub struct LdtkValidationFinishedEvent {
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LdtkEntityRegistryKey {
    pub layer_identifier: Option<String>,
    pub entity_identifier: Option<String>,
}

pub type LdtkEntitySpawner =
    Box<dyn Fn(&mut World, Entity, &LdtkEntitySpawnContext) + Send + Sync + 'static>;

#[derive(Resource, Default)]
pub struct LdtkEntityRegistry {
    pub spawners: HashMap<LdtkEntityRegistryKey, LdtkEntitySpawner>,
}

impl LdtkEntityRegistry {
    pub fn register_bundle<B>(&mut self, identifier: impl Into<String>)
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.register_bundle_for_layer_optional::<B>(None, Some(identifier.into()));
    }

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

    pub fn register_default_bundle_for_layer<B>(&mut self, layer_identifier: impl Into<String>)
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.register_bundle_for_layer_optional::<B>(Some(layer_identifier.into()), None);
    }

    pub fn register_default_bundle<B>(&mut self)
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.register_bundle_for_layer_optional::<B>(None, None);
    }

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

    pub fn register_spawner(
        &mut self,
        identifier: impl Into<String>,
        spawner: impl Fn(&mut World, Entity, &LdtkEntitySpawnContext) + Send + Sync + 'static,
    ) {
        self.register_spawner_for_layer_optional(None, Some(identifier.into()), spawner);
    }

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
