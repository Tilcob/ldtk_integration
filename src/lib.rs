//! # ldtk_integration
//!
//! A Bevy plugin for loading and managing LDtk levels in 2D games.
//!
//! Rendering and asset loading are handled by [`bevy_ecs_ldtk`]; this crate
//! adds a game-oriented API on top: a world/level/layer catalog, typed entity
//! registration, IntGrid collision rules, level transitions with spawn-point
//! resolution, and tile-animation metadata.
//!
//! ## Quick start
//!
//! ```no_run
//! use bevy::prelude::*;
//! use ldtk_integration::{GameLdtkPlugin, LdtkConfig};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(GameLdtkPlugin::new(
//!             LdtkConfig::default()
//!                 .with_world_asset_path("worlds/map.ldtk")
//!                 .with_solid_int_grid_values([1]),
//!         ))
//!         .run();
//! }
//! ```
//!
//! ## Feature flags
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `tilemap` | ✅ | Tile-animation adapter via `bevy_ecs_tilemap` |
//! | `external-level-fs` | ✅ | Read external `.ldtkl` files from disk (not WASM) |
//!
//! ## Crate layout
//!
//! Everything public is re-exported from the crate root. Use
//! `ldtk_integration::prelude::*` or import individual items directly.

pub mod ldtk;

// ── Plugins ───────────────────────────────────────────────────────────────────
pub use ldtk::plugins::GameLdtkPlugin;
pub use ldtk::level_manager::LevelManagerPlugin;

// ── Command extensions ────────────────────────────────────────────────────────
pub use ldtk::commands::LdtkAppExt;
pub use ldtk::commands::LdtkCommandExt;

// ── Config & rules ────────────────────────────────────────────────────────────
pub use ldtk::core::LdtkConfig;
pub use ldtk::core::LdtkCollisionRule;

// ── External level source ─────────────────────────────────────────────────────
pub use ldtk::core::ExternalLevelSource;
pub use ldtk::core::LdtkExternalLevelSource;
#[cfg(feature = "external-level-fs")]
pub use ldtk::core::FsExternalLevelSource;
#[cfg(feature = "external-level-fs")]
pub use ldtk::core::external_level_path;

// ── System sets ───────────────────────────────────────────────────────────────
pub use ldtk::core::LdtkLoadSet;

// ── Resources: load state & catalog ──────────────────────────────────────────
pub use ldtk::core::LdtkLoadState;
pub use ldtk::core::LdtkLoadStatus;
pub use ldtk::core::LdtkLoadStats;
pub use ldtk::core::LdtkValidationReport;
pub use ldtk::core::LdtkValidationIssue;
pub use ldtk::core::LdtkRuntimeState;
pub use ldtk::core::LdtkTransitionState;
pub use ldtk::core::LdtkMapCatalog;
pub use ldtk::core::LdtkCollisionCatalog;
pub use ldtk::core::LdtkEntityCatalog;
pub use ldtk::core::LdtkCommandQueue;
pub use ldtk::core::LdtkEntityRegistry;

// ── Catalog data types ────────────────────────────────────────────────────────
pub use ldtk::core::LdtkWorldInfo;
pub use ldtk::core::LdtkLevelInfo;
pub use ldtk::core::LdtkLayerInfo;
pub use ldtk::core::LdtkLayerType;
pub use ldtk::core::LdtkTilesetInfo;
pub use ldtk::core::LdtkCollisionLayerInfo;
pub use ldtk::core::LdtkCollisionCell;
pub use ldtk::core::LdtkNeighbor;
pub use ldtk::core::LdtkDirection;
pub use ldtk::core::LdtkWorldLayout;
pub use ldtk::core::LdtkSpawnPoint;
pub use ldtk::core::LdtkTileKey;
pub use ldtk::core::LdtkTileMetadata;
pub use ldtk::core::LdtkTileAnimation;
pub use ldtk::core::LdtkTileAnimationFrame;
pub use ldtk::core::LdtkTileAnimator;

// ── Field values ──────────────────────────────────────────────────────────────
pub use ldtk::core::LdtkFieldValue;
pub use ldtk::core::LdtkFieldAccess;
pub use ldtk::core::LdtkEntityReference;
pub use ldtk::core::LdtkTilesetRect;

// ── Entity types ──────────────────────────────────────────────────────────────
pub use ldtk::core::LdtkEntitySpawnContext;
pub use ldtk::core::LdtkImportedEntity;
pub use ldtk::core::LdtkEntityMarker;
pub use ldtk::core::LdtkEntityRegistryKey;
pub use ldtk::core::LdtkEntitySpawner;

// ── Marker components ─────────────────────────────────────────────────────────
pub use ldtk::core::LdtkWorldRoot;
pub use ldtk::core::LdtkPersistent;
pub use ldtk::core::LdtkCollider;
pub use ldtk::core::LdtkTileCollision;

// ── Events ────────────────────────────────────────────────────────────────────
pub use ldtk::core::LdtkSpawnWorldEvent;
pub use ldtk::core::LdtkMapLoadedEvent;
pub use ldtk::core::LdtkLevelActivatedEvent;
pub use ldtk::core::LdtkWorldUnloadedEvent;
pub use ldtk::core::LdtkValidationFinishedEvent;

// ── Level manager ─────────────────────────────────────────────────────────────
pub use ldtk::level_manager::LdtkLevelManagerConfig;
pub use ldtk::level_manager::LdtkPlayerLocator;
pub use ldtk::level_manager::CurrentLdtkLevel;
pub use ldtk::level_manager::PendingLdtkLevelTransition;
pub use ldtk::level_manager::LevelTransitionStatus;
pub use ldtk::level_manager::LevelTransitionState;
pub use ldtk::level_manager::LevelTransitionRequest;
pub use ldtk::level_manager::LdtkLevelReadyEvent;
pub use ldtk::level_manager::LdtkCollisionReadyEvent;
pub use ldtk::level_manager::LdtkLevelPlayer;
pub use ldtk::level_manager::LdtkLevelScoped;
pub use ldtk::level_manager::advance_tile_animation;

/// Re-exports the most commonly used items for glob imports.
///
/// ```no_run
/// use ldtk_integration::prelude::*;
/// ```
pub mod prelude {
    pub use super::*;
}
