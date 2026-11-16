//! Drupal YAML metadata extractor (`*.services.yml`, `*.routing.yml`, etc.).

use crate::model::{SymbolKind, SymbolRecord};
use serde_yaml_ng::Value;

/// Supported Drupal YAML metadata file kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YamlKind {
    Services,
    Routing,
    Info,
    Permissions,
    Libraries,
    Schema,
}

impl YamlKind {
    /// Detects the Drupal YAML kind from a file path, if supported.
    #[must_use]
    pub fn from_path(path: &str) -> Option<Self> {
        let lower = path.to_ascii_lowercase();
        if lower.ends_with(".services.yml") {
            Some(Self::Services)
        } else if lower.ends_with(".routing.yml") {
            Some(Self::Routing)
        } else if lower.ends_with(".info.yml") {
            Some(Self::Info)
        } else if lower.ends_with(".permissions.yml") {
            Some(Self::Permissions)
        } else if lower.ends_with(".libraries.yml") {
            Some(Self::Libraries)
        } else if lower.ends_with(".schema.yml") {
            Some(Self::Schema)
        } else {
            None
        }
    }
}

/// Parses a Drupal YAML file into [`SymbolRecord`] entries.
///
/// Malformed YAML files return an empty list rather than aborting the whole index.
#[must_use]
pub fn parse_yaml_file(rel_path: &str, source: &str) -> Vec<SymbolRecord> {
    let Some(kind) = YamlKind::from_path(rel_path) else {
        return Vec::new();
    };
    let Ok(doc) = serde_yaml_ng::from_str::<Value>(source) else {
        return Vec::new();
    };
    let Some(root) = doc.as_mapping() else {
        return Vec::new();
    };

    match kind {
        YamlKind::Services => extract_services(rel_path, source, root),
        YamlKind::Routing => extract_routing(rel_path, source, root),
        YamlKind::Info => extract_info(rel_path, source, root),
        YamlKind::Permissions => extract_permissions(rel_path, source, root),
        YamlKind::Libraries => extract_libraries(rel_path, source, root),
        YamlKind::Schema => extract_schema(rel_path, source, root),
    }
}

fn extract_services(
    rel_path: &str,
    source: &str,
    root: &serde_yaml_ng::Mapping,
) -> Vec<SymbolRecord> {
    let Some(services) = mapping_get(root, "services").and_then(Value::as_mapping) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (k, v) in services {
        let Some(id) = k.as_str() else {
            continue;
        };
        if id == "_defaults" || id == "_instanceof" {
            continue;
        }
        let line = find_key_line(source, id);
        let (class_name, sig, attrs) = if let Some(map) = v.as_mapping() {
            let class_name = mapping_str(map, "class").unwrap_or_default();
            let alias = mapping_str(map, "alias").unwrap_or_default();
            let tags = extract_tag_names(map);
            let sig = if !class_name.is_empty() {
                format!("{id} -> {class_name}")
            } else if !alias.is_empty() {
                format!("{id} -> alias:{alias}")
            } else {
                id.to_string()
            };
            (class_name, sig, tags)
        } else if let Some(alias) = v.as_str() {
            let target = alias.trim_start_matches('@');
            (
                target.to_string(),
                format!("{id} -> alias:{target}"),
                String::new(),
            )
        } else {
            (String::new(), id.to_string(), String::new())
        };

        out.push(SymbolRecord {
            kind: SymbolKind::Service,
            name: id.to_string(),
            fqn: if class_name.is_empty() {
                id.to_string()
            } else {
                class_name
            },
            file: rel_path.to_string(),
            line,
            end_line: line,
            signature: sig,
            doc: String::new(),
            attrs,
        });
    }
    out
}

fn extract_routing(
    rel_path: &str,
    source: &str,
    root: &serde_yaml_ng::Mapping,
) -> Vec<SymbolRecord> {
    let mut out = Vec::new();
    for (k, v) in root {
        let Some(name) = k.as_str() else {
            continue;
        };
        let Some(map) = v.as_mapping() else {
            continue;
        };
        let path = mapping_str(map, "path").unwrap_or_default();
        let defaults = mapping_get(map, "defaults").and_then(Value::as_mapping);
        let handler = defaults
            .and_then(|d| {
                mapping_str(d, "_controller")
                    .or_else(|| mapping_str(d, "_form"))
                    .or_else(|| mapping_str(d, "_entity_form"))
                    .or_else(|| mapping_str(d, "_entity_list"))
            })
            .unwrap_or_default();
        let title = defaults
            .and_then(|d| mapping_str(d, "_title"))
            .unwrap_or_default();

        let sig = match (path.is_empty(), handler.is_empty()) {
            (false, false) => format!("{path} -> {handler}"),
            (false, true) => path.clone(),
            (true, false) => handler,
            (true, true) => name.to_string(),
        };
        let line = find_key_line(source, name);
        out.push(SymbolRecord {
            kind: SymbolKind::Route,
            name: name.to_string(),
            fqn: if path.is_empty() {
                name.to_string()
            } else {
                path
            },
            file: rel_path.to_string(),
            line,
            end_line: line,
            signature: sig,
            doc: title,
            attrs: String::new(),
        });
    }
    out
}

fn extract_info(rel_path: &str, _source: &str, root: &serde_yaml_ng::Mapping) -> Vec<SymbolRecord> {
    let name = mapping_str(root, "name").unwrap_or_else(|| file_stem_name(rel_path));
    let info_type = mapping_str(root, "type").unwrap_or_default();
    let desc = mapping_str(root, "description").unwrap_or_default();
    let deps = mapping_get(root, "dependencies")
        .and_then(Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();

    let machine = file_stem_name(rel_path);
    vec![SymbolRecord {
        kind: SymbolKind::Info,
        name: machine.clone(),
        fqn: format!("{info_type}:{machine}"),
        file: rel_path.to_string(),
        line: 1,
        end_line: 1,
        signature: format!("{name} ({info_type})"),
        doc: desc,
        attrs: deps,
    }]
}

fn extract_permissions(
    rel_path: &str,
    source: &str,
    root: &serde_yaml_ng::Mapping,
) -> Vec<SymbolRecord> {
    let mut out = Vec::new();
    for (k, v) in root {
        let Some(perm) = k.as_str() else {
            continue;
        };
        let (title, desc) = v
            .as_mapping()
            .map(|m| {
                (
                    mapping_str(m, "title").unwrap_or_default(),
                    mapping_str(m, "description").unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        let line = find_key_line(source, perm);
        out.push(SymbolRecord {
            kind: SymbolKind::Permission,
            name: perm.to_string(),
            fqn: perm.to_string(),
            file: rel_path.to_string(),
            line,
            end_line: line,
            signature: title,
            doc: desc,
            attrs: String::new(),
        });
    }
    out
}

fn extract_libraries(
    rel_path: &str,
    source: &str,
    root: &serde_yaml_ng::Mapping,
) -> Vec<SymbolRecord> {
    let module = file_stem_name(rel_path);
    let mut out = Vec::new();
    for (k, v) in root {
        let Some(lib) = k.as_str() else {
            continue;
        };
        let deps = v
            .as_mapping()
            .and_then(|m| mapping_get(m, "dependencies"))
            .and_then(Value::as_sequence)
            .map(|seq| {
                seq.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let fqn = format!("{module}/{lib}");
        let line = find_key_line(source, lib);
        out.push(SymbolRecord {
            kind: SymbolKind::Library,
            name: lib.to_string(),
            fqn: fqn.clone(),
            file: rel_path.to_string(),
            line,
            end_line: line,
            signature: fqn,
            doc: String::new(),
            attrs: deps,
        });
    }
    out
}

fn extract_schema(
    rel_path: &str,
    source: &str,
    root: &serde_yaml_ng::Mapping,
) -> Vec<SymbolRecord> {
    let mut out = Vec::new();
    for (k, v) in root {
        let Some(key) = k.as_str() else {
            continue;
        };
        let (schema_type, label) = v
            .as_mapping()
            .map(|m| {
                (
                    mapping_str(m, "type").unwrap_or_default(),
                    mapping_str(m, "label").unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        let line = find_key_line(source, key);
        out.push(SymbolRecord {
            kind: SymbolKind::Schema,
            name: key.to_string(),
            fqn: key.to_string(),
            file: rel_path.to_string(),
            line,
            end_line: line,
            signature: if schema_type.is_empty() {
                key.to_string()
            } else {
                format!("{key}: {schema_type}")
            },
            doc: label,
            attrs: String::new(),
        });
    }
    out
}

fn mapping_get<'a>(map: &'a serde_yaml_ng::Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.to_string()))
}

fn mapping_str(map: &serde_yaml_ng::Mapping, key: &str) -> Option<String> {
    mapping_get(map, key)
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn extract_tag_names(map: &serde_yaml_ng::Mapping) -> String {
    mapping_get(map, "tags")
        .and_then(Value::as_sequence)
        .map(|seq| {
            seq.iter()
                .filter_map(|item| item.as_mapping().and_then(|m| mapping_str(m, "name")))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn file_stem_name(rel_path: &str) -> String {
    let base = rel_path.rsplit('/').next().unwrap_or(rel_path);
    base.split('.').next().unwrap_or(base).to_string()
}

fn find_key_line(source: &str, key: &str) -> usize {
    for (idx, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(after) = trimmed.strip_prefix(key)
            && after.starts_with(':')
        {
            return idx + 1;
        }
        let quoted_single = format!("'{key}':");
        let quoted_double = format!("\"{key}\":");
        if trimmed.starts_with(&quoted_single) || trimmed.starts_with(&quoted_double) {
            return idx + 1;
        }
    }
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_services_and_routing() {
        let services_yml = "services:\n  entity_type.manager:\n    class: Drupal\\Core\\Entity\\EntityTypeManager\n    tags:\n      - { name: needs_destruction }\n";
        let recs = parse_yaml_file("core/core.services.yml", services_yml);
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].kind, SymbolKind::Service);
        assert_eq!(recs[0].name, "entity_type.manager");
        assert_eq!(recs[0].fqn, r"Drupal\Core\Entity\EntityTypeManager");
        assert_eq!(recs[0].line, 2);

        let routing_yml = "entity.node.canonical:\n  path: '/node/{node}'\n  defaults:\n    _controller: '\\Drupal\\node\\Controller\\NodeViewController::view'\n    _title: 'Node'\n";
        let routes = parse_yaml_file("core/modules/node/node.routing.yml", routing_yml);
        assert_eq!(routes.len(), 1);
        assert_eq!(routes[0].kind, SymbolKind::Route);
        assert_eq!(routes[0].fqn, "/node/{node}");
        assert_eq!(routes[0].line, 1);
    }
}
