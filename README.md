# ldtk_integration

Eine Bevy-Dependency für LDtk-Level-Loading in 2D-Spielen. Das Rendering kommt von [`bevy_ecs_ldtk`](https://github.com/Trouv/bevy_ecs_ldtk); dieses Crate legt darüber eine spielnahe API für Runtime-State, Kataloge, Entity-Registrierung, Collision und Level-Transitions.

**Versionen:** Bevy `0.18`, bevy_ecs_ldtk `0.14`, Rust Edition 2024

---

## Inhaltsverzeichnis

- [Installation](#installation)
- [Feature Flags](#feature-flags)
- [Schnellstart](#schnellstart)
- [GameLdtkPlugin](#gameldtkplugin)
- [LevelManagerPlugin](#levelmanagerplugin)
- [Entity-Registrierung](#entity-registrierung)
- [Collision](#collision)
- [Layer-Filter](#layer-filter)
- [Tile-Animationen](#tile-animationen)
- [Load-State und Validierung](#load-state-und-validierung)
- [API-Referenz](#api-referenz)
- [Beispiel](#beispiel)

---

## Installation

```toml
# Cargo.toml des Spiels
[dependencies]
ldtk_integration = { path = "../ldtk_integration" }
bevy = "0.18.1"
```

Per Git:

```toml
ldtk_integration = { git = "https://github.com/<user>/ldtk_integration.git" }
```

---

## Feature Flags

| Feature | Default | Beschreibung |
|---------|---------|--------------|
| `tilemap` | ✅ an | Tilemap-Animations-Adapter über `bevy_ecs_tilemap` |
| `external-level-fs` | ✅ an | Liest externe `.ldtkl` Level vom Dateisystem (kein WASM) |

WASM-Build ohne Dateisystem-Zugriff:

```toml
ldtk_integration = { path = "...", default-features = false, features = ["tilemap"] }
```

---

## Schnellstart

LDtk-Dateien liegen relativ zu `assets/`. Eine Datei unter `assets/worlds/map.ldtk` wird als `"worlds/map.ldtk"` angegeben.

```rust
use bevy::prelude::*;
use ldtk_integration::{GameLdtkPlugin, LdtkConfig};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GameLdtkPlugin::new(
            LdtkConfig::default()
                .with_world_asset_path("worlds/map.ldtk")
                .with_solid_int_grid_values([1]),
        ))
        .run();
}
```

---

## GameLdtkPlugin

Das Kern-Plugin. Muss immer registriert sein.

```rust
use ldtk_integration::{GameLdtkPlugin, LdtkConfig};

app.add_plugins(GameLdtkPlugin::new(LdtkConfig::default()));
// oder mit Defaults:
app.add_plugins(GameLdtkPlugin::default());
```

Registriert folgende Ressourcen: `LdtkConfig`, `LdtkRuntimeState`, `LdtkLoadState`, `LdtkValidationReport`, `LdtkMapCatalog`, `LdtkCollisionCatalog`, `LdtkEntityCatalog`, `LdtkCommandQueue`, `LdtkEntityRegistry`, `LdtkExternalLevelSource`.

### LdtkConfig

Builder-API zur Konfiguration des Plugins:

```rust
LdtkConfig::default()
    // Pfad zur .ldtk-Datei, relativ zu assets/
    .with_world_asset_path("worlds/map.ldtk")

    // Basis-Pfad für externe .ldtkl-Level (default: "assets")
    .with_asset_root("assets")

    // Bestimmte IntGrid-Werte als solid markieren
    // (ohne Angabe: alle Werte != 0 gelten als solid)
    .with_solid_int_grid_values([1, 2])

    // Präzise Collision-Regeln (überschreiben int_grid_solid_values)
    .with_collision_rules([
        LdtkCollisionRule::solid(1).for_layer("Collision"),
        LdtkCollisionRule::sensor(2, "water").for_layer("Gameplay"),
    ])

    // Nur diese Layer katalogisieren
    .include_layers(["Collision", "Entities"])

    // Diese Layer überspringen
    .exclude_layers(["Debug", "Notes"])

    // Externe .ldtkl-Dateien nicht einlesen
    .without_external_level_catalog()

    // Warnings zu Errors promoten (setzt LdtkLoadState auf Error)
    .with_strict_validation()

    // Validierung komplett deaktivieren
    .without_validation()

    // Kein Warn-Log für nicht registrierte LDtk-Entities
    .without_unregistered_entity_warnings()
```

### Commands

Alle Commands sind über `LdtkCommandExt` auf `Commands` verfügbar:

```rust
use ldtk_integration::LdtkCommandExt;

// World laden (überschreibt laufende World)
commands.spawn_ldtk_world("worlds/map.ldtk");

// Level wechseln (LevelSelection, kein Spieler-Teleport)
commands.change_ldtk_level("Level_01");

// Alias für change_ldtk_level
commands.change_level("Level_01");

// Aktuelle World neu laden
commands.reload_ldtk_world();

// World entladen
commands.unload_ldtk_world();

// Level-Transition mit Spawnpunkt (benötigt LevelManagerPlugin)
commands.transition_to_ldtk_level("Level_02", Some("Entrance_A"));
commands.transition_to_ldtk_level("Level_02", None::<String>);
```

### App Extensions

Entity-Registrierung über `LdtkAppExt`:

```rust
use ldtk_integration::LdtkAppExt;

// Bundle registrieren (Default::default() wird als Basis genutzt)
app.register_ldtk_entity::<PlayerBundle>("Player");

// Bundle auf Layer + Entity-Identifier einschränken
app.register_ldtk_entity_for_layer::<ChestBundle>("Objects", "Chest");

// Spawner-Funktion registrieren
app.register_ldtk_entity_spawner("Door", my_door_spawner);

// Spawner auf Layer + Entity-Identifier einschränken
app.register_ldtk_entity_spawner_for_layer("Objects", "Key", my_key_spawner);
```

---

## LevelManagerPlugin

Optionales Plugin für Level-Transitions mit Spawnpunkt-Logik und automatischem Entity-Cleanup. Benötigt `GameLdtkPlugin`.

```rust
use ldtk_integration::{GameLdtkPlugin, LevelManagerPlugin};

app.add_plugins(GameLdtkPlugin::default())
   .add_plugins(LevelManagerPlugin);
```

### Transition auslösen

```rust
// Zu einem Level wechseln, Spieler landet an "Entrance_A"
commands.transition_to_ldtk_level("Dungeon_02", Some("Entrance_A"));

// Spawnpunkt automatisch wählen (PlayerSpawn → erster Spawnpunkt → Fallback)
commands.transition_to_ldtk_level("Dungeon_02", None::<String>);
```

### Spawnpunkt-Auflösung

Der Manager sucht in dieser Reihenfolge:

1. Entity mit `identifier == spawn_id` oder Tag `spawn_id` (wenn angegeben)
2. Entity mit `identifier == "PlayerSpawn"` oder Tag `"PlayerSpawn"`
3. Erster Spawnpunkt im Level
4. `Vec2::ZERO` wenn `allow_missing_spawnpoints: true`
5. `LevelTransitionStatus::Failed` wenn kein Spawnpunkt gefunden

Als Spawnpunkt gilt jede LDtk-Entity deren Identifier `"spawn"` enthält oder die den Tag `"spawn"` trägt.

### Spieler-Teleport

```rust
// Option A: Marker-Komponente
commands.spawn((Player, LdtkLevelPlayer, Transform::default(), GlobalTransform::default()));

// Option B: Explizit per Ressource (überschreibt Marker-Suche)
commands.insert_resource(LdtkPlayerLocator { entity: Some(player_entity) });
```

### Konfiguration

```rust
app.insert_resource(LdtkLevelManagerConfig {
    default_spawn_tag: "PlayerSpawn".to_string(),
    default_spawn_identifier: "PlayerSpawn".to_string(),
    allow_missing_spawnpoints: false,
    enable_tile_animation_adapter: false,
    ..Default::default()
});
```

### Events

| Event | Inhalt | Wann |
|-------|--------|------|
| `LdtkLevelReadyEvent` | `level_identifier`, `spawn_id`, `position` | Spieler wurde teleportiert |
| `LdtkCollisionReadyEvent` | `level_identifier`, `cells` | Collision-Daten des Levels fertig |
| `LdtkMapLoadedEvent` | `world_identifier` | World vollständig geladen |
| `LdtkLevelActivatedEvent` | `level_identifier` | Level per `change_ldtk_level` aktiviert |
| `LdtkWorldUnloadedEvent` | — | World entladen |

### Transition-State

```rust
fn watch_state(state: Res<LevelTransitionState>) {
    match state.status {
        LevelTransitionStatus::Idle => {}
        LevelTransitionStatus::WaitingForSpawn => { /* Ladebildschirm anzeigen */ }
        LevelTransitionStatus::Ready => { /* Ladebildschirm ausblenden */ }
        LevelTransitionStatus::Failed => {
            error!("Transition fehlgeschlagen: {:?}", state.error);
        }
    }
}
```

### Persistenz und Cleanup

Beim Levelwechsel werden alle Entities despawnt, die:
- `LdtkEntityMarker` mit dem alten Level tragen, **oder**
- `LdtkLevelScoped { level_identifier }` mit dem alten Level tragen

Ausnahmen (werden nicht despawnt):
- Entities mit `LdtkPersistent`
- Entities mit `LdtkLevelPlayer`

```rust
// Entity bleibt über Levelwechsel erhalten
commands.entity(my_entity).insert(LdtkPersistent);

// Entity wird beim Verlassen von "Level_01" despawnt
commands.entity(my_entity).insert(LdtkLevelScoped {
    level_identifier: "Level_01".to_string(),
});
```

---

## Entity-Registrierung

### Bundle (einfach)

```rust
#[derive(Bundle, Default)]
struct ChestBundle {
    chest: Chest,
    sprite: Sprite,
}

app.register_ldtk_entity::<ChestBundle>("Chest");
```

`Transform` und `GlobalTransform` werden automatisch aus der LDtk-Entity-Position gesetzt. `LdtkEntityMarker` wird ebenfalls automatisch hinzugefügt.

### Spawner (flexibel)

```rust
app.register_ldtk_entity_spawner("Door", |world: &mut World, entity: Entity, ctx: &LdtkEntitySpawnContext| {
    let target_level = ctx.field_str("target_level").unwrap_or("").to_string();
    let target_spawn = ctx.field_str("target_spawn").unwrap_or("").to_string();

    world.entity_mut(entity).insert(Door { target_level, target_spawn });
});
```

### LdtkEntitySpawnContext

Enthält alle Informationen zur LDtk-Entity beim Spawn:

| Feld | Typ | Beschreibung |
|------|-----|--------------|
| `entity_iid` | `String` | Eindeutige LDtk-ID der Entity-Instanz |
| `entity_identifier` | `String` | Definition-Name (z.B. `"Door"`) |
| `world_identifier` | `Option<String>` | Name der LDtk-World |
| `level_identifier` | `Option<String>` | Name des Levels |
| `layer_identifier` | `Option<String>` | Name des Layers |
| `position` | `Vec2` | Pixelposition in der World |
| `grid_position` | `IVec2` | Gitter-Position im Layer |
| `size` | `Vec2` | Größe der Entity in Pixeln |
| `pivot` | `Vec2` | Pivot-Punkt (0.0–1.0) |
| `tags` | `Vec<String>` | LDtk-Entity-Tags |
| `tile` | `Option<LdtkTileMetadata>` | Optionale Tile-Darstellung der Entity |
| `field_values` | `HashMap<String, LdtkFieldValue>` | Alle Custom Fields |

### Field-Zugriff

`LdtkEntitySpawnContext` und `LdtkImportedEntity` implementieren beide `LdtkFieldAccess`:

```rust
ctx.field("my_field")              // Option<&LdtkFieldValue>
ctx.field_bool("active")           // Option<bool>
ctx.field_i64("damage")            // Option<i64>
ctx.field_f64("speed")             // Option<f64>
ctx.field_str("label")             // Option<&str>
```

### LdtkFieldValue

```rust
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
```

---

## Collision

### Konfiguration

```rust
LdtkConfig::default()
    // Alle Werte != 0 werden solid (Standard wenn keine Regeln gesetzt)
    .with_solid_int_grid_values([1, 2])

    // Oder präzise Regeln pro Layer und Wert
    .with_collision_rules([
        LdtkCollisionRule::solid(1).for_layer("Collision"),
        LdtkCollisionRule::sensor(2, "water").for_layer("Gameplay"),
        LdtkCollisionRule::sensor(3, "damage"),  // gilt für alle Layer
    ])
```

### Zur Laufzeit auslesen

```rust
fn build_colliders(
    mut commands: Commands,
    catalog: Res<LdtkCollisionCatalog>,
    mut ready: MessageReader<LdtkCollisionReadyEvent>,
) {
    for event in ready.read() {
        let cells: Vec<_> = catalog.cells.iter()
            .filter(|cell| cell.level_identifier == event.level_identifier)
            .collect();

        for cell in cells {
            if cell.solid {
                // Rapier/Avian Collider erzeugen bei cell.grid_position
            }
            if cell.sensor {
                // Sensor-Trigger mit cell.tag erzeugen
            }
        }
    }
}
```

### LdtkCollisionCell

| Feld | Typ | Beschreibung |
|------|-----|--------------|
| `level_identifier` | `String` | Level in dem die Zelle liegt |
| `level_iid` | `String` | LDtk-IID des Levels |
| `layer_identifier` | `String` | Layer-Name |
| `grid_position` | `IVec2` | Gitter-Position |
| `value` | `i32` | IntGrid-Wert |
| `solid` | `bool` | Ist physikalisch solid |
| `sensor` | `bool` | Ist Sensor/Trigger |
| `tag` | `Option<String>` | Semantischer Tag (z.B. `"water"`) |

Entities mit passender IntGrid-Zelle bekommen automatisch `LdtkCollider { solid, sensor }`.

---

## Layer-Filter

```rust
// Nur diese Layer in den Katalog aufnehmen
LdtkConfig::default().include_layers(["Collision", "Entities", "Gameplay"])

// Diese Layer überspringen (kombinierbar mit include_layers)
LdtkConfig::default().exclude_layers(["Debug", "Notes", "Editor"])
```

Gefilterte Layer erscheinen weder im `LdtkMapCatalog` noch werden ihre Entities oder Tiles verarbeitet.

---

## Tile-Animationen

LDtk hat keine native Tile-Animation. Dieses Crate liest eine Konvention aus Tile Custom Data:

```
anim=1,2,3;fps=8
frames=1@0.1,2@0.1,3@0.2;repeat=false
```

**Format:**
- `anim=<ids>` oder `frames=<ids>`: Komma-getrennte Tile-IDs
- `fps=<n>`: Frames pro Sekunde (einheitliche Dauer)
- `<id>@<sekunden>`: Individuelle Dauer pro Frame
- `repeat=false`: Animation stoppt beim letzten Frame (Standard: `true`)

Gefundene Animationen stehen in `LdtkMapCatalog::tile_animations` und `LdtkTileMetadata::animation`. `LdtkTileAnimator` tickt automatisch über `GameLdtkPlugin`.

**Tilemap-Adapter** (experimentell, benötigt Feature `tilemap`):

```rust
app.insert_resource(LdtkLevelManagerConfig {
    enable_tile_animation_adapter: true,
    ..Default::default()
});
```

Wendet laufende `LdtkTileAnimator`-Zustände auf `TileTextureIndex` von `bevy_ecs_tilemap` an. Muss mit einem echten LDtk-Testlevel verifiziert werden.

---

## Load-State und Validierung

```rust
fn debug_ldtk(
    load: Res<LdtkLoadState>,
    report: Res<LdtkValidationReport>,
) {
    match load.status {
        LdtkLoadStatus::NotLoaded => {}
        LdtkLoadStatus::Loading => {}
        LdtkLoadStatus::Ready => {
            info!("{} Level(s) geladen", load.stats.levels);
        }
        LdtkLoadStatus::Error => {
            error!("LDtk Fehler: {:?}", load.errors);
        }
    }

    for warning in &report.warnings {
        warn!("[{}] {}", warning.code, warning.message);
    }
}
```

### LdtkLoadStats

```rust
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
```

### Validierungscodes

| Code | Bedeutung |
|------|-----------|
| `external_level_not_cataloged` | Externes `.ldtkl` Level ohne Layer-Daten im Katalog |
| `external_level_wasm_unsupported` | Externes Level auf WASM nicht lesbar |
| `missing_spawn_point` | Level hat keinen Spawnpunkt |
| `unregistered_entity` | LDtk-Entity hat kein registriertes Bundle/Spawner |
| `missing_tileset_path` | Layer referenziert Tileset ohne Pfad |
| `transition_level_missing` | Transition-Ziel nicht im Katalog |
| `transition_spawn_missing` | Angeforderter Spawnpunkt nicht gefunden |

`LdtkConfig::with_strict_validation()` stuft alle Codes als Errors ein und setzt `LdtkLoadStatus::Error`.

---

## API-Referenz

### Ressourcen

| Ressource | Beschreibung |
|-----------|--------------|
| `LdtkConfig` | Konfiguration (World-Pfad, Collision, Filter, Validierung) |
| `LdtkRuntimeState` | Aktive World, aktives Level, geladene Level-IIDs |
| `LdtkLoadState` | Status (NotLoaded/Loading/Ready/Error), Statistiken, Warn-/Fehlerliste |
| `LdtkValidationReport` | Strukturierte Warnings und Errors mit Code und Nachricht |
| `LdtkMapCatalog` | Worlds, Level, Layer, Tilesets, Tiles, Spawnpoints, Entity-Snapshots, Tile-Animationen |
| `LdtkCollisionCatalog` | IntGrid-Zellen mit Collision-Typ, Layer-Zusammenfassungen |
| `LdtkEntityCatalog` | IID → Bevy-Entity-Zuordnung, Entity-Snapshots |
| `LdtkEntityRegistry` | Registrierte Spawner/Bundles |
| `LdtkExternalLevelSource` | Austauschbare I/O-Strategie für externe Level |
| `LdtkCommandQueue` | Interne Command-Queue (nicht direkt nutzen) |
| `CurrentLdtkLevel` | Aktuelles Level-Identifier + IID (LevelManagerPlugin) |
| `PendingLdtkLevelTransition` | Laufende Transition (LevelManagerPlugin) |
| `LevelTransitionState` | Status der Transition + Fehlermeldung (LevelManagerPlugin) |
| `LdtkLevelManagerConfig` | Konfiguration des LevelManagerPlugins |
| `LdtkPlayerLocator` | Explizite Player-Entity für Teleport (optional) |

### Komponenten

| Komponente | Beschreibung |
|------------|--------------|
| `LdtkWorldRoot` | Markiert die Root-Entity der geladenen LDtk-World |
| `LdtkEntityMarker` | Auf jeder gespawnten LDtk-Entity: Definition-ID, Level, World |
| `LdtkImportedEntity` | Snapshot aller LDtk-Felder auf der Bevy-Entity |
| `LdtkCollider` | `{ solid: bool, sensor: bool }` — von Collision-Capture gesetzt |
| `LdtkTileCollision` | Tile-spezifische Collision-Info |
| `LdtkTileAnimation` | Animations-Frames + repeat-Flag |
| `LdtkTileAnimator` | Laufender Animations-State (frame_index, timer) |
| `LdtkPersistent` | Opt-in: Entity überlebt Levelwechsel |
| `LdtkLevelPlayer` | Markiert Spieler-Entity für automatischen Teleport |
| `LdtkLevelScoped` | Entity wird beim Verlassen des angegebenen Levels despawnt |

### System Sets (Reihenfolge)

```
LdtkLoadSet::Commands       ← process_ldtk_commands
LdtkLoadSet::Catalog        ← refresh_map_catalog, sync_level_events
LdtkLoadSet::Capture        ← collision, entity_instances, entity_behaviors
LdtkLoadSet::LevelTransitions ← handle_requests, finalize_transition
LdtkLoadSet::Animation      ← tile_animators
```

Eigene Systeme können relativ zu diesen Sets geordnet werden:

```rust
app.add_systems(Update,
    my_system.after(LdtkLoadSet::Catalog).before(LdtkLoadSet::Capture)
);
```

### LdtkMapCatalog — wichtige Methoden

```rust
// Level per Identifier oder IID nachschlagen (O(1))
catalog.level_by_id_or_iid("Level_01")
catalog.level_by_id_or_iid("abc-123-iid")

// IID → Identifier (O(1))
catalog.identifier_for_iid("abc-123-iid")

// Manuell einfügen (hält IID-Index synchron)
catalog.insert_level_info(level_info)

catalog.is_empty()
```

### Externe Level-Quelle austauschen

Für WASM oder eigenes I/O:

```rust
use ldtk_integration::{ExternalLevelSource, LdtkExternalLevelSource};

struct MyLevelSource;

impl ExternalLevelSource for MyLevelSource {
    fn load(&self, asset_root: &str, world_path: &str, rel_path: &str) -> Option<String> {
        // Eigene Logik: HTTP-Fetch, eingebettete Bytes, etc.
        None
    }
}

app.insert_resource(LdtkExternalLevelSource(Some(Box::new(MyLevelSource))));
```

---

## Beispiel

`examples/stealth_doors.rs` zeigt ein vollständiges Setup für ein Stealth-Puzzle-Game:

- `GameLdtkPlugin` mit Collision-Regeln
- `LevelManagerPlugin` mit Spieler-Teleport
- `Door`-Entity aus LDtk-Feldern spawnen (`target_level`, `target_spawn`)
- Level-Transition beim Betreten einer Tür auslösen
- Events auswerten (`LdtkLevelReadyEvent`, `LdtkCollisionReadyEvent`)
- Transition-Fehler loggen

```
cargo run --example stealth_doors
```

Das Beispiel benötigt eine `assets/worlds/stealth.ldtk`-Datei. Ohne sie startet die App, lädt aber keine Level — alle Events bleiben aus.

---

## Tests

```powershell
cargo test                      # alle Unit-Tests
cargo test --no-default-features  # ohne tilemap + fs (WASM-Pfad)
cargo fmt                       # Formatierung
cargo clippy --all-targets      # Lints
```

Testabdeckung: Field-Helper, Layer-Filter, Tile-Animations-Parser, Tile-ID-Berechnung, Collision-Regeln, Spawnpunkt-Auflösung, Transition-State, Catalog-Index, Validierungslogik.
