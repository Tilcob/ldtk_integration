use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Resource)]
pub struct LdtkConfig {
    pub world_asset_path: Option<String>,
    pub streaming_radius_in_levels: i32,
    pub culling_enabled: bool,
    pub use_hybrid_wfc: bool,
    pub parallax_enabled: bool,
    pub default_seed: u64,
}

impl Default for LdtkConfig {
    fn default() -> Self {
        Self {
            world_asset_path: None,
            streaming_radius_in_levels: 1,
            culling_enabled: true,
            use_hybrid_wfc: false,
            parallax_enabled: true,
            default_seed: 0,
        }
    }
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkRuntimeState {
    pub active_world: Option<String>,
    pub active_world_root: Option<Entity>,
    pub active_level: Option<String>,
    pub active_biome: Option<String>,
    pub seed: Option<u64>,
    pub transition: LdtkTransitionState,
    pub loaded_levels: HashSet<String>,
    pub persistent_entities: HashSet<Entity>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum LdtkTransitionState {
    #[default]
    Idle,
    Requested,
    Loading,
    Active,
    Unloading,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkMapCatalog {
    pub worlds: HashMap<String, LdtkWorldInfo>,
    pub levels: HashMap<String, LdtkLevelInfo>,
    pub portals: Vec<LdtkPortalLink>,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkCollisionCatalog {
    pub layers: HashMap<String, LdtkCollisionLayerInfo>,
    pub cells: Vec<LdtkCollisionCell>,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkCollisionLayerInfo {
    pub level_identifier: String,
    pub layer_identifier: String,
    pub layer_type: String,
    pub solid_cells: usize,
    pub tile_cells: usize,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkCollisionCell {
    pub level_identifier: String,
    pub layer_identifier: String,
    pub grid_position: IVec2,
    pub solid: bool,
    pub source: String,
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
    pub identifier: String,
    pub world_identifier: String,
    pub path: String,
    pub width: i32,
    pub height: i32,
    pub neighbors: Vec<LdtkNeighbor>,
    pub spawn_points: Vec<LdtkSpawnPoint>,
    pub tiles: Vec<LdtkTileMetadata>,
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

#[derive(Debug, Clone, Default)]
pub enum LdtkWorldLayout {
    #[default]
    Free,
    GridVania,
    LinearHorizontal,
    LinearVertical,
    Manual,
}

#[derive(Debug, Clone)]
pub struct LdtkSpawnPoint {
    pub identifier: String,
    pub position: Vec2,
    pub level_identifier: String,
    pub tags: Vec<String>,
}

impl Default for LdtkSpawnPoint {
    fn default() -> Self {
        Self {
            identifier: String::new(),
            position: Vec2::ZERO,
            level_identifier: String::new(),
            tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LdtkPortalLink {
    pub source_level: String,
    pub target_level: String,
    pub source_portal_id: String,
    pub target_portal_id: String,
    pub target_spawn: Option<String>,
}

impl Default for LdtkPortalLink {
    fn default() -> Self {
        Self {
            source_level: String::new(),
            target_level: String::new(),
            source_portal_id: String::new(),
            target_portal_id: String::new(),
            target_spawn: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LdtkEntityReference {
    pub entity_iid: String,
    pub layer_iid: String,
    pub level_iid: String,
    pub world_iid: String,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkTileMetadata {
    pub tileset_identifier: String,
    pub tile_id: i32,
    pub source_position: IVec2,
    pub rotation_degrees: u16,
    pub flip_x: bool,
    pub flip_y: bool,
    pub weight: f32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LdtkFieldValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Color(Color),
    Point(Option<Vec2>),
    IntPoint(IVec2),
    EntityRef(LdtkEntityReference),
    Array(Vec<LdtkFieldValue>),
    Null,
}

impl Default for LdtkFieldValue {
    fn default() -> Self {
        Self::Null
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
    pub field_values: HashMap<String, LdtkFieldValue>,
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
pub struct LdtkSpawnedLevel {
    pub level_identifier: String,
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

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkRenderLayer {
    pub layer_name: String,
    pub z_index: f32,
    pub parallax_scale: Vec2,
}

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkChunkMarker {
    pub chunk_x: i32,
    pub chunk_y: i32,
    pub level_identifier: String,
}

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkWfcSeed {
    pub seed: u64,
    pub biome: Option<String>,
}

#[derive(Debug, Clone, Component, Default)]
pub struct LdtkWorldState {
    pub seed: u64,
    pub biome: Option<String>,
    pub generated: bool,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkRuleDatabase {
    pub signatures: HashMap<String, LdtkTileSignature>,
    pub compatibility: HashMap<String, Vec<String>>,
    pub weights: HashMap<String, f32>,
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkRuleExtractionReport {
    pub levels_scanned: usize,
    pub tiles_scanned: usize,
    pub observations_created: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct LdtkTileSignature {
    pub tileset_identifier: String,
    pub tile_id: i32,
    pub rotation_degrees: u16,
    pub flip_x: bool,
    pub flip_y: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LdtkRuleExtractionSettings {
    pub include_auto_layers: bool,
    pub include_int_grid: bool,
    pub include_entities_as_constraints: bool,
    pub neighborhood_radius: u8,
}

#[derive(Debug, Clone)]
pub struct LdtkRuleObservation {
    pub center: LdtkTileSignature,
    pub north: Option<LdtkTileSignature>,
    pub south: Option<LdtkTileSignature>,
    pub east: Option<LdtkTileSignature>,
    pub west: Option<LdtkTileSignature>,
    pub weight: f32,
}

impl Default for LdtkRuleObservation {
    fn default() -> Self {
        Self {
            center: LdtkTileSignature::default(),
            north: None,
            south: None,
            east: None,
            west: None,
            weight: 1.0,
        }
    }
}

#[derive(Debug, Clone, Resource, Default)]
pub struct LdtkGeneratedMapRequests {
    pub pending: Vec<LdtkGeneratedMapRequest>,
}

#[derive(Debug, Clone)]
pub struct LdtkGeneratedMapRequest {
    pub seed: u64,
    pub biome: Option<String>,
    pub parent_world: Option<String>,
    pub target_level: Option<String>,
}

impl Default for LdtkGeneratedMapRequest {
    fn default() -> Self {
        Self {
            seed: 0,
            biome: None,
            parent_world: None,
            target_level: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub enum LdtkCommand {
    #[default]
    None,
    SpawnWorld {
        world_path: String,
    },
    ChangeLevel {
        level_identifier: String,
    },
    GenerateWfcLevel {
        seed: u64,
        biome: Option<String>,
    },
    RequestPortalTransition {
        source_level: String,
        target_level: String,
        portal_id: String,
    },
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
pub struct LdtkChangeLevelEvent {
    pub level_identifier: String,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkGenerateWfcLevelEvent {
    pub seed: u64,
    pub biome: Option<String>,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkPortalTransitionEvent {
    pub source_level: String,
    pub target_level: String,
    pub portal_id: String,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkMapLoadedEvent {
    pub world_identifier: String,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkMapUnloadedEvent {
    pub world_identifier: String,
}

#[derive(Debug, Clone, Message)]
pub struct LdtkLevelActivatedEvent {
    pub level_identifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LdtkEntityRegistryKey {
    pub layer_identifier: Option<String>,
    pub entity_identifier: Option<String>,
}

pub type LdtkEntitySpawner = Box<dyn Fn(&mut World, Entity, &LdtkEntitySpawnContext) + Send + Sync + 'static>;

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

    pub fn register_bundle_for_layer<B>(&mut self, layer_identifier: impl Into<String>, identifier: impl Into<String>)
    where
        B: Bundle + Default + Send + Sync + 'static,
    {
        self.register_bundle_for_layer_optional::<B>(Some(layer_identifier.into()), Some(identifier.into()));
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
            layer_identifier: layer_identifier.clone(),
            entity_identifier: entity_identifier.clone(),
        };
        let registry_identifier = entity_identifier.unwrap_or_else(|| "<default>".to_string());

        self.spawners.insert(
            key,
            Box::new(move |world: &mut World, entity: Entity, context: &LdtkEntitySpawnContext| {
                let _ = registry_identifier.as_str();
                let _ = context.position;
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
            }),
        );
    }

    pub fn resolve(&self, layer_identifier: Option<&str>, entity_identifier: &str) -> Option<&LdtkEntitySpawner> {
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

impl From<&bevy_ecs_ldtk::ldtk::FieldInstance> for LdtkFieldValue {
    fn from(value: &bevy_ecs_ldtk::ldtk::FieldInstance) -> Self {
        match &value.value {
            bevy_ecs_ldtk::ldtk::FieldValue::Int(v) => Self::Int(i64::from(v.unwrap_or_default())),
            bevy_ecs_ldtk::ldtk::FieldValue::Float(v) => Self::Float(f64::from(v.unwrap_or_default())),
            bevy_ecs_ldtk::ldtk::FieldValue::Bool(v) => Self::Bool(*v),
            bevy_ecs_ldtk::ldtk::FieldValue::String(v) => {
                Self::String(v.clone().unwrap_or_default())
            }
            bevy_ecs_ldtk::ldtk::FieldValue::Color(v) => Self::Color(*v),
            bevy_ecs_ldtk::ldtk::FieldValue::FilePath(v) => {
                Self::String(v.clone().unwrap_or_default())
            }
            bevy_ecs_ldtk::ldtk::FieldValue::Enum(v) => Self::String(v.clone().unwrap_or_default()),
            bevy_ecs_ldtk::ldtk::FieldValue::Tile(_) => Self::Null,
            bevy_ecs_ldtk::ldtk::FieldValue::EntityRef(v) => Self::EntityRef(
                v.as_ref()
                    .map(LdtkEntityReference::from)
                    .unwrap_or_default(),
            ),
            bevy_ecs_ldtk::ldtk::FieldValue::Point(v) => {
                Self::Point(v.map(|p| Vec2::new(p.x as f32, p.y as f32)))
            }
            bevy_ecs_ldtk::ldtk::FieldValue::Ints(v) => Self::Array(
                v.iter()
                    .map(|entry| match entry {
                        Some(i) => Self::Int(i64::from(*i)),
                        None => Self::Null,
                    })
                    .collect(),
            ),
            bevy_ecs_ldtk::ldtk::FieldValue::Floats(v) => Self::Array(
                v.iter()
                    .map(|entry| match entry {
                        Some(f) => Self::Float(f64::from(*f)),
                        None => Self::Null,
                    })
                    .collect(),
            ),
            bevy_ecs_ldtk::ldtk::FieldValue::Bools(v) => {
                Self::Array(v.iter().map(|entry| Self::Bool(*entry)).collect())
            }
            bevy_ecs_ldtk::ldtk::FieldValue::Strings(v) => Self::Array(
                v.iter()
                    .map(|entry| match entry {
                        Some(text) => Self::String(text.clone()),
                        None => Self::Null,
                    })
                    .collect(),
            ),
            bevy_ecs_ldtk::ldtk::FieldValue::Colors(v) => {
                Self::Array(v.iter().map(|entry| Self::Color(*entry)).collect())
            }
            bevy_ecs_ldtk::ldtk::FieldValue::FilePaths(v) => Self::Array(
                v.iter()
                    .map(|entry| match entry {
                        Some(text) => Self::String(text.clone()),
                        None => Self::Null,
                    })
                    .collect(),
            ),
            bevy_ecs_ldtk::ldtk::FieldValue::Enums(v) => Self::Array(
                v.iter()
                    .map(|entry| match entry {
                        Some(text) => Self::String(text.clone()),
                        None => Self::Null,
                    })
                    .collect(),
            ),
            bevy_ecs_ldtk::ldtk::FieldValue::Tiles(_) => Self::Null,
            bevy_ecs_ldtk::ldtk::FieldValue::EntityRefs(v) => Self::Array(
                v.iter()
                    .map(|entry| {
                        entry
                            .as_ref()
                            .map(|reference| Self::EntityRef(LdtkEntityReference::from(reference)))
                            .unwrap_or(Self::Null)
                    })
                    .collect(),
            ),
            bevy_ecs_ldtk::ldtk::FieldValue::Points(v) => Self::Array(
                v.iter()
                    .map(|entry| match entry {
                        Some(point) => Self::Point(Some(Vec2::new(point.x as f32, point.y as f32))),
                        None => Self::Null,
                    })
                    .collect(),
            ),
        }
    }
}

impl From<&bevy_ecs_ldtk::prelude::EntityInstance> for LdtkEntitySpawnContext {
    fn from(value: &bevy_ecs_ldtk::prelude::EntityInstance) -> Self {
        let field_values = value
            .field_instances
            .iter()
            .map(|field| (field.identifier.clone(), LdtkFieldValue::from(field)))
            .collect();

        Self {
            entity_iid: value.iid.clone(),
            entity_identifier: value.identifier.clone(),
            world_identifier: None,
            level_identifier: None,
            layer_identifier: None,
            position: Vec2::new(value.px.x as f32, value.px.y as f32),
            grid_position: value.grid,
            size: Vec2::new(value.width as f32, value.height as f32),
            pivot: value.pivot,
            tags: value.tags.clone(),
            tile: value.tile.as_ref().map(|tile| LdtkTileMetadata {
                tileset_identifier: String::new(),
                tile_id: tile.tileset_uid,
                source_position: IVec2::new(tile.x, tile.y),
                rotation_degrees: 0,
                flip_x: false,
                flip_y: false,
                weight: 1.0,
                tags: Vec::new(),
            }),
            field_values,
        }
    }
}











