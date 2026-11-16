//! PHP AST extractor using `tree-sitter-php`.

use crate::error::{IndexError, Result};
use crate::model::{SymbolKind, SymbolRecord};
use tree_sitter::{Node, Parser};

/// Returns `true` if `path` has a PHP/Drupal code extension.
#[must_use]
pub fn is_php_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [
        ".php", ".module", ".inc", ".install", ".theme", ".profile", ".engine",
    ]
    .iter()
    .any(|ext| lower.ends_with(ext))
}

/// Parses a single PHP source file into [`SymbolRecord`] entries.
///
/// # Errors
/// Returns [`IndexError::Parser`] if the tree-sitter PHP grammar fails to load.
pub fn parse_php_file(rel_path: &str, source: &str) -> Result<Vec<SymbolRecord>> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
        .map_err(|e| IndexError::Parser(e.to_string()))?;

    let Some(tree) = parser.parse(source, None) else {
        return Ok(Vec::new());
    };

    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut current_ns = String::new();
    let root = tree.root_node();

    walk_top_level(root, rel_path, bytes, &mut current_ns, &mut out);
    Ok(out)
}

fn walk_top_level(
    node: Node<'_>,
    rel_path: &str,
    bytes: &[u8],
    current_ns: &mut String,
    out: &mut Vec<SymbolRecord>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "namespace_definition" => {
                if let Some(name_node) = child.child_by_field_name("name") {
                    *current_ns = node_text(name_node, bytes).trim().to_string();
                }
                if let Some(body) = child.child_by_field_name("body") {
                    walk_top_level(body, rel_path, bytes, current_ns, out);
                }
            }
            "class_declaration" => {
                extract_type(child, SymbolKind::Class, rel_path, bytes, current_ns, out);
            }
            "interface_declaration" => {
                extract_type(
                    child,
                    SymbolKind::Interface,
                    rel_path,
                    bytes,
                    current_ns,
                    out,
                );
            }
            "trait_declaration" => {
                extract_type(child, SymbolKind::Trait, rel_path, bytes, current_ns, out);
            }
            "enum_declaration" => {
                extract_type(child, SymbolKind::Enum, rel_path, bytes, current_ns, out);
            }
            "function_definition" => {
                extract_function(child, rel_path, bytes, current_ns, out);
            }
            _ => {
                if child.child_count() > 0 && child.kind() != "compound_statement" {
                    walk_top_level(child, rel_path, bytes, current_ns, out);
                }
            }
        }
    }
}

fn extract_type(
    node: Node<'_>,
    kind: SymbolKind,
    rel_path: &str,
    bytes: &[u8],
    current_ns: &str,
    out: &mut Vec<SymbolRecord>,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = node_text(name_node, bytes).trim().to_string();
    let fqn = qualify(current_ns, &name);
    let attrs = collect_attributes(node, bytes);
    let doc = preceding_docblock(node, bytes);
    let signature = header_before_body(node, bytes);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    out.push(SymbolRecord {
        kind,
        name,
        fqn: fqn.clone(),
        file: rel_path.to_string(),
        line,
        end_line,
        signature,
        doc,
        attrs,
    });

    if let Some(body) = node.child_by_field_name("body") {
        let mut cursor = body.walk();
        for member in body.children(&mut cursor) {
            if member.kind() == "method_declaration" {
                extract_method(member, rel_path, bytes, &fqn, out);
            }
        }
    }
}

fn extract_method(
    node: Node<'_>,
    rel_path: &str,
    bytes: &[u8],
    owner_fqn: &str,
    out: &mut Vec<SymbolRecord>,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = node_text(name_node, bytes).trim().to_string();
    let fqn = format!("{owner_fqn}::{name}");
    let attrs = collect_attributes(node, bytes);
    let doc = preceding_docblock(node, bytes);
    let signature = header_before_body(node, bytes);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    out.push(SymbolRecord {
        kind: SymbolKind::Method,
        name: name.clone(),
        fqn: fqn.clone(),
        file: rel_path.to_string(),
        line,
        end_line,
        signature: signature.clone(),
        doc: doc.clone(),
        attrs: attrs.clone(),
    });

    for hook_name in extract_hook_attribute_names(&attrs) {
        out.push(SymbolRecord {
            kind: SymbolKind::Hook,
            name: hook_name,
            fqn: fqn.clone(),
            file: rel_path.to_string(),
            line,
            end_line,
            signature: signature.clone(),
            doc: doc.clone(),
            attrs: attrs.clone(),
        });
    }
}

fn extract_function(
    node: Node<'_>,
    rel_path: &str,
    bytes: &[u8],
    current_ns: &str,
    out: &mut Vec<SymbolRecord>,
) {
    let Some(name_node) = node.child_by_field_name("name") else {
        return;
    };
    let name = node_text(name_node, bytes).trim().to_string();
    let fqn = qualify(current_ns, &name);
    let attrs = collect_attributes(node, bytes);
    let doc = preceding_docblock(node, bytes);
    let signature = header_before_body(node, bytes);
    let line = node.start_position().row + 1;
    let end_line = node.end_position().row + 1;

    out.push(SymbolRecord {
        kind: SymbolKind::Function,
        name: name.clone(),
        fqn: fqn.clone(),
        file: rel_path.to_string(),
        line,
        end_line,
        signature: signature.clone(),
        doc: doc.clone(),
        attrs: attrs.clone(),
    });

    if name.starts_with("hook_") {
        out.push(SymbolRecord {
            kind: SymbolKind::Hook,
            name,
            fqn,
            file: rel_path.to_string(),
            line,
            end_line,
            signature,
            doc,
            attrs,
        });
    }
}

fn qualify(ns: &str, name: &str) -> String {
    if ns.is_empty() {
        name.to_string()
    } else {
        format!("{ns}\\{name}")
    }
}

fn node_text<'a>(node: Node<'_>, bytes: &'a [u8]) -> &'a str {
    node.utf8_text(bytes).unwrap_or("")
}

fn collect_attributes(node: Node<'_>, bytes: &[u8]) -> String {
    let mut parts = Vec::new();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_list" {
            let text = collapse_ws(node_text(child, bytes));
            if !text.is_empty() {
                parts.push(text);
            }
        }
    }
    parts.join(" ")
}

fn header_before_body(node: Node<'_>, bytes: &[u8]) -> String {
    let start = node.start_byte();
    let end = node
        .child_by_field_name("body")
        .map_or_else(|| node.end_byte(), |b| b.start_byte());
    let slice = bytes.get(start..end).unwrap_or(&[]);
    let raw = std::str::from_utf8(slice).unwrap_or("");
    collapse_ws(raw.trim_end_matches('{').trim_end_matches(';'))
}

fn preceding_docblock(node: Node<'_>, bytes: &[u8]) -> String {
    let mut prev = node.prev_sibling();
    while let Some(p) = prev {
        if p.kind() == "comment" {
            let txt = node_text(p, bytes).trim();
            if txt.starts_with("/**") {
                return clean_docblock(txt);
            }
            prev = p.prev_sibling();
            continue;
        }
        if p.kind() == "attribute_list" {
            prev = p.prev_sibling();
            continue;
        }
        break;
    }
    String::new()
}

fn clean_docblock(raw: &str) -> String {
    let inner = raw
        .trim_start_matches("/**")
        .trim_end_matches("*/")
        .lines()
        .map(|l| l.trim().trim_start_matches('*').trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    collapse_ws(&inner)
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extracts hook names from `#[Hook('...')]` or `#[\Drupal\Core\Hook\Attribute\Hook("...")]`,
/// ignoring `#[ReorderHook(...)]` or `#[RemoveHook(...)]`.
fn extract_hook_attribute_names(attrs: &str) -> Vec<String> {
    let mut hooks = Vec::new();
    let mut offset = 0;
    while let Some(rel) = attrs[offset..].find("Hook(") {
        let abs = offset + rel;
        let prev_char = attrs[..abs].chars().next_back();
        let is_hook_attr = prev_char.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_');
        let after = &attrs[abs + 5..];
        if is_hook_attr {
            let trimmed = after.trim_start();
            if let Some(quote) = trimmed.chars().next().filter(|&c| c == '\'' || c == '"')
                && let Some(end_q) = trimmed[1..].find(quote)
            {
                let val = &trimmed[1..=end_q];
                if !val.is_empty() {
                    let normalized = if val.starts_with("hook_") {
                        val.to_string()
                    } else {
                        format!("hook_{val}")
                    };
                    hooks.push(normalized);
                }
            }
        }
        offset = abs + 5;
    }
    hooks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_class_method_and_hook_attribute() {
        let src = r"<?php

namespace Drupal\node\Hook;

use Drupal\Core\Hook\Attribute\Hook;

/**
 * Node hook implementations.
 */
class NodeHooks {
  /**
   * Implements hook_entity_presave().
   */
  #[Hook('entity_presave')]
  #[ReorderHook('entity_presave', class: Other::class, method: 'm')]
  public function entityPresave(object $entity): void {
  }
}
";
        let records = parse_php_file("core/modules/node/src/Hook/NodeHooks.php", src).unwrap();
        assert!(records.iter().any(|r| r.kind == SymbolKind::Class
            && r.fqn == r"Drupal\node\Hook\NodeHooks"
            && r.doc.contains("Node hook implementations.")));
        assert!(records.iter().any(|r| r.kind == SymbolKind::Method
            && r.fqn == r"Drupal\node\Hook\NodeHooks::entityPresave"));
        let hooks: Vec<_> = records
            .iter()
            .filter(|r| r.kind == SymbolKind::Hook)
            .collect();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].name, "hook_entity_presave");
        assert_eq!(hooks[0].fqn, r"Drupal\node\Hook\NodeHooks::entityPresave");
    }

    #[test]
    fn extracts_procedural_hook_function() {
        let src = "<?php\n\n/**\n * Act on entity.\n */\nfunction hook_entity_insert($entity) {}\n";
        let records = parse_php_file("core/lib/Drupal/Core/Entity/entity.api.php", src).unwrap();
        assert!(
            records
                .iter()
                .any(|r| r.kind == SymbolKind::Hook && r.name == "hook_entity_insert")
        );
    }
}
