//! Domain models for indexed PHP symbols and Drupal YAML metadata.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Kind of indexed symbol or Drupal metadata entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SymbolKind {
    Class,
    Interface,
    Trait,
    Enum,
    Method,
    Function,
    Hook,
    Service,
    Route,
    Info,
    Permission,
    Library,
    Schema,
}

impl SymbolKind {
    /// Returns the lowercase canonical string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Class => "class",
            Self::Interface => "interface",
            Self::Trait => "trait",
            Self::Enum => "enum",
            Self::Method => "method",
            Self::Function => "function",
            Self::Hook => "hook",
            Self::Service => "service",
            Self::Route => "route",
            Self::Info => "info",
            Self::Permission => "permission",
            Self::Library => "library",
            Self::Schema => "schema",
        }
    }

    /// Parses a kind string (case-insensitive).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "class" => Some(Self::Class),
            "interface" => Some(Self::Interface),
            "trait" => Some(Self::Trait),
            "enum" => Some(Self::Enum),
            "method" => Some(Self::Method),
            "function" | "fn" => Some(Self::Function),
            "hook" => Some(Self::Hook),
            "service" | "services" => Some(Self::Service),
            "route" | "routing" => Some(Self::Route),
            "info" => Some(Self::Info),
            "permission" | "permissions" => Some(Self::Permission),
            "library" | "libraries" => Some(Self::Library),
            "schema" => Some(Self::Schema),
            _ => None,
        }
    }
}

impl fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A single indexed symbol or metadata record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolRecord {
    /// Entry kind.
    pub kind: SymbolKind,
    /// Short name (e.g. `EntityTypeManager`, `entity_type.manager`, `hook_entity_presave`).
    pub name: String,
    /// Fully qualified name or canonical key.
    pub fqn: String,
    /// Relative path inside the Drupal repository.
    pub file: String,
    /// 1-based start line.
    pub line: usize,
    /// 1-based end line.
    pub end_line: usize,
    /// Concise signature or summary line.
    pub signature: String,
    /// Extracted docblock or YAML description.
    pub doc: String,
    /// Attributes or extra structured metadata (e.g. `#[Hook('...')]`, service tags).
    pub attrs: String,
}

/// Per-kind symbol count in [`IndexStats`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KindCount {
    /// Canonical `SymbolKind` name.
    pub kind: String,
    /// Number of records of this kind.
    pub count: usize,
}

/// Summary statistics for the current index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexStats {
    /// Git commit SHA indexed.
    pub commit_sha: String,
    /// RFC-3339 or Unix timestamp of the last index run.
    pub indexed_at: String,
    /// Repository URL.
    pub repo_url: String,
    /// Total number of indexed records.
    pub total_symbols: usize,
    /// Number of distinct indexed files.
    pub total_files: usize,
    /// Counts broken down by `SymbolKind`.
    pub by_kind: Vec<KindCount>,
}
