# LDtk Integration (Bevy) — Architektur-Scaffold

Dieses Repository enthält ein modulares Scaffold für die Integration von LDtk-Projekten in Bevy-Apps. Ziel ist eine wiederverwendbare, erweiterbare Architektur, die später als eigenes Crate oder per Copy-Paste in andere Projekte übernommen werden kann.

Kernziele:
- Saubere Trennung von Import, Rendering, Collision, Entity-Handling, Level-Streaming und WFC-Vorbereitung
- ECS-konforme Ressourcen- und Nachrichtenmodelle
- Vorbereitung für regelbasierte WFC-Extraktion und deterministische Generierung
- Ergonomische Public-API für Sprites/Levels/Transitions

Inhalt dieser Implementierung
- Basale Plugin-Struktur: `GameLdtkPlugin` plus feingranulare Sub-Plugins
- Ressourcen / Kataloge: `LdtkMapCatalog`, `LdtkCollisionCatalog`, `LdtkRuleDatabase`, `LdtkEntityCatalog` usw.
- Snapshot-basierte Entity-Import-Modelle und Registry für automatische Bundle-Spawner
- Erste WFC-Regel-Extraktions-Pipeline (Regelkatalog aus Tiles + Nachbarschaften)
- Verbesserte Nachbarschafts-Heuristik: Aus LDtk-Hinweisen und World-Positionen werden gerichtete Nachbarn mit Kosten (Heuristik) berechnet — nützlich für Streaming, Übergänge und WFC-Kompatibilitäten

Schnellstart — Beispiel

1) App initialisieren und Plugin einbinden

```rust
use bevy::prelude::*;
use ldtk_integration::GameLdtkPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GameLdtkPlugin::default())
        // hier weitere Plugin / Registrierung
        .run();
}
```

2) Commands API (konzeptionell)
- `commands.spawn_ldtk_world("path/to/world.ldtk")` — lädt ein LDtk-Projekt (über den Bevy AssetServer) als World-Root
- `commands.change_level("level_iid_or_identifier")` — wechselt aktives Level (nutzt `LevelSelection`)
- `commands.generate_wfc_level(seed)` — fügt einen WFC-Generierungs-Request zur Queue hinzu

Praktischer Testablauf (lokal)
1. Lege eine LDtk-Datei bereit (z.B. `assets/worlds/my_world.ldtk`).
2. Starte die App (im Projekt-Root):

```powershell
cargo run --release
```

3. In einem Startup-System (oder interaktiv in einer System-Initialisierung) kannst du ein World-Spawn-Request in die Queue setzen:

```rust
commands.spawn_ldtk_world("assets/worlds/my_world.ldtk");
```

4. Beobachte `LdtkMapCatalog` und `LdtkRuleExtractionReport` Ressourcen während des Laufs — die Nachbarschafts-Heuristik füllt jetzt `LdtkLevelInfo::neighbors` mit Richtung und Kosten, basierend auf LDtk-Hinweisen und World-Koordinaten.

API / Module Übersicht
- `src/lib.rs` — Crate-Root & Re-exports
- `src/ldtk/core.rs` — Datentypen: LevelInfo, WorldInfo, Neighbor-Model, Rule-DB, Entity-Snapshots
- `src/ldtk/commands.rs` — ergonomische Erweiterungen (spawn world, change level, generate wfc)
- `src/ldtk/plugins.rs` — Plugins und Systeme (Import, Rendering, Collision, Entity, Streaming, WFC, MapManagement)

Was wurde verbessert (wichtig für Tests)
- Die Nachbarschafts-Heuristik nutzt nun die LDtk-eigene `__neighbours`-Angabe und versucht, anhand `world_x/world_y` zuverlässige Richtungen zu inferieren. Die Knoten im `LdtkMapCatalog` bekommen Richtungs-Informationen + eine einfache Kostenabschätzung:
  - Kardinal: cost = 1.0
  - Diagonalhinweis: cost ≈ 1.4 (als Hinweis auf diagonale Berührung)
  - Überlappende Levels (`o`): cost = 0.2 (sehr nahe)
  - Tiefenwechsel (`<`/`>`): cost = 1.5 (Penalität für different depth)

Diese Informationen werden in `LdtkRuleDatabase.compatibility` verwendet, wenn die WFC-Extraktion läuft.

Tipps für weiteres Debugging / Entwicklung
- Öffne im Lauf die Ressourcen (`LdtkMapCatalog`, `LdtkRuleExtractionReport`) per Debug-Log oder in einem Development-UI-System
- Falls dein Projekt LinearLayouts nutzt, haben die Levels häufig `world_x/world_y == -1`. In dem Fall werden Nachbarn primär aus LDtk-Hinweisen abgeleitet (oder bleiben leer)
- Wenn du zusätzliche Heuristiken brauchst (z.B. Portal-Metadaten oder Spawn-Entitäten als Übergänge), erweitere `compute_level_neighbors_from_json` im Plugin um weitere Regeln

Wenn du möchtest, schreibe ich jetzt kleine Unit-Tests oder ein kleines Beispiel-System, das beim Start eine LDtk-Datei lädt und die berechneten Nachbarn ausgibt.

