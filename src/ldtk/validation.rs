//! Catalog validation: cross-checks the built [`LdtkMapCatalog`] against the
//! [`LdtkEntityRegistry`] and [`LdtkConfig`], collecting issues into an
//! [`LdtkValidationReport`]. Severity routing (warning vs. error under
//! `strict_validation`) lives entirely in [`LdtkValidationReport::push`].

use crate::ldtk::core::{LdtkConfig, LdtkEntityRegistry, LdtkMapCatalog, LdtkValidationReport};

pub(crate) fn validate_catalog(
    catalog: &LdtkMapCatalog,
    registry: &LdtkEntityRegistry,
    config: &LdtkConfig,
    report: &mut LdtkValidationReport,
) {
    report.clear();
    let strict = config.strict_validation;

    for level in catalog.levels.values() {
        if level.external_path.is_some() && level.tiles.is_empty() && level.entities.is_empty() {
            report.push(
                strict,
                "external_level_not_cataloged",
                format!(
                    "Level '{}' references an external .ldtkl file. bevy_ecs_ldtk can load it, but this metadata catalog only sees embedded layer data.",
                    level.identifier
                ),
            );
        }

        #[cfg(target_arch = "wasm32")]
        if level.external_path.is_some() {
            report.push(
                strict,
                "external_level_wasm_unsupported",
                format!(
                    "Level '{}' references an external .ldtkl file. The metadata catalog skips external levels on wasm32; use embedded levels or custom IO.",
                    level.identifier
                ),
            );
        }

        if level.spawn_points.is_empty() {
            report.push(
                strict,
                "missing_spawn_point",
                format!(
                    "Level '{}' has no entity tagged/named as spawn.",
                    level.identifier
                ),
            );
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
                    report.push(
                        strict,
                        "unregistered_entity",
                        format!(
                            "LDtk entity '{}' in level '{}' has no registered bundle/spawner.",
                            entity.entity_identifier, level.identifier
                        ),
                    );
                }
            }
        }
    }

    for layer in catalog.layers.values() {
        if layer.tileset_uid.is_some() && layer.tileset_rel_path.is_none() {
            report.push(
                strict,
                "missing_tileset_path",
                format!(
                    "Layer '{}' in level '{}' references a tileset without a relative path.",
                    layer.identifier, layer.level_identifier
                ),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ldtk::core::{LdtkLevelInfo, LdtkSpawnPoint};

    #[test]
    fn missing_spawn_point_is_a_warning_unless_strict() {
        let mut catalog = LdtkMapCatalog::default();
        catalog.insert_level_info(LdtkLevelInfo {
            identifier: "Level_A".to_string(),
            ..Default::default()
        });

        let registry = LdtkEntityRegistry::default();
        let mut report = LdtkValidationReport::default();

        validate_catalog(&catalog, &registry, &LdtkConfig::default(), &mut report);
        assert!(!report.has_errors());
        assert!(
            report
                .warnings
                .iter()
                .any(|i| i.code == "missing_spawn_point")
        );

        let mut report = LdtkValidationReport::default();
        validate_catalog(
            &catalog,
            &registry,
            &LdtkConfig::default().with_strict_validation(),
            &mut report,
        );
        assert!(report.has_errors());
        assert!(
            report
                .errors
                .iter()
                .any(|i| i.code == "missing_spawn_point")
        );
    }

    #[test]
    fn registered_spawn_point_passes() {
        let mut catalog = LdtkMapCatalog::default();
        catalog.insert_level_info(LdtkLevelInfo {
            identifier: "Level_A".to_string(),
            spawn_points: vec![LdtkSpawnPoint {
                identifier: "PlayerSpawn".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        });

        let registry = LdtkEntityRegistry::default();
        let mut report = LdtkValidationReport::default();
        validate_catalog(
            &catalog,
            &registry,
            &LdtkConfig::default().without_unregistered_entity_warnings(),
            &mut report,
        );

        assert!(!report.has_errors());
        assert!(report.warnings.is_empty());
    }
}
