//! Fast CLI indexer and query tool for `drupal/drupal` (`main`).

mod db;
mod error;
mod git;
mod indexer;
mod model;
mod php;
mod yaml;

use crate::db::IndexDb;
use crate::error::{IndexError, Result};
use crate::git::{DEFAULT_BRANCH, DEFAULT_REPO_URL, head_sha, sync_repo};
use crate::indexer::{default_data_dir, index_repository};
use crate::model::{SymbolKind, SymbolRecord};
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Debug, Parser)]
#[command(
    name = "drupal-index",
    version,
    about = "Fast local indexer for drupal/drupal (main) — PHP AST + Drupal YAML + SQLite FTS5"
)]
struct Cli {
    /// Directory storing the shallow Drupal mirror and `index.sqlite`.
    #[arg(long, env = "DRUPAL_INDEX_DATA_DIR", global = true)]
    data_dir: Option<PathBuf>,

    /// Emit machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Clone or pull `drupal/drupal` (main) and rebuild the `SQLite` index if HEAD changed.
    Sync {
        /// Skip `git clone` / `git fetch` and index the existing local mirror.
        #[arg(long)]
        no_pull: bool,
        /// Force re-indexing even when HEAD SHA matches the database.
        #[arg(long)]
        force: bool,
        /// Override upstream git URL.
        #[arg(long, default_value = DEFAULT_REPO_URL)]
        repo_url: String,
        /// Override upstream branch.
        #[arg(long, default_value = DEFAULT_BRANCH)]
        branch: String,
    },
    /// Full-text and prefix search across indexed PHP symbols and Drupal YAML metadata.
    Search {
        /// Search query (FTS5 tokens or symbol substring).
        query: String,
        /// Filter by kind (`class`, `interface`, `trait`, `enum`, `method`, `function`, `hook`, `service`, `route`, `info`, `permission`, `library`, `schema`).
        #[arg(long)]
        kind: Option<String>,
        /// Maximum results to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Look up a PHP class, interface, trait, enum, method, or function by name or FQN.
    Symbol {
        /// Symbol name or FQN (e.g. `EntityTypeManager` or `Drupal\Core\Entity\EntityTypeManager`).
        name: String,
        /// Maximum results to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Look up Drupal hooks (`#[Hook('...')]` and `hook_*`).
    Hook {
        /// Hook name with or without `hook_` prefix.
        name: String,
        /// Maximum results to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Look up Drupal services from `*.services.yml`.
    Service {
        /// Service ID or search term (e.g. `entity_type.manager`).
        query: String,
        /// Maximum results to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Look up Drupal routes from `*.routing.yml`.
    Route {
        /// Route name or path term (e.g. `entity.node.canonical`).
        query: String,
        /// Maximum results to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show indexed commit SHA, timestamp, and counts by kind.
    Stats,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let data_dir = match cli.data_dir {
        Some(dir) => dir,
        None => default_data_dir()?,
    };
    let repo_dir = data_dir.join("repo");
    let db_path = data_dir.join("index.sqlite");

    match cli.command {
        Command::Sync {
            no_pull,
            force,
            repo_url,
            branch,
        } => {
            if !no_pull {
                sync_repo(&repo_dir, &repo_url, &branch)?;
            }
            let sha = head_sha(&repo_dir)?;
            let mut db = IndexDb::open(&db_path)?;
            let prev_sha = db.get_meta("commit_sha")?.unwrap_or_default();
            if !force && prev_sha == sha {
                if cli.json {
                    println!(
                        "{}",
                        serde_json::json!({"status": "up_to_date", "commit_sha": sha})
                    );
                } else {
                    println!("up to date at {sha}");
                }
                return Ok(());
            }
            let indexed = index_repository(&repo_dir, &mut db, &sha, &repo_url)?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({"status": "indexed", "commit_sha": sha, "records": indexed})
                );
            } else {
                println!("indexed {indexed} records at {sha}");
            }
        }
        Command::Search { query, kind, limit } => {
            let parsed_kind = match kind {
                Some(ref raw) => Some(SymbolKind::parse(raw).ok_or_else(|| {
                    IndexError::InvalidArg(format!("unknown symbol kind: {raw}"))
                })?),
                None => None,
            };
            let db = IndexDb::open(&db_path)?;
            let hits = db.search(&query, parsed_kind, limit)?;
            print_records(&hits, &repo_dir, cli.json)?;
        }
        Command::Symbol { name, limit } => {
            let db = IndexDb::open(&db_path)?;
            let hits = db.find_symbol(&name, limit)?;
            print_records(&hits, &repo_dir, cli.json)?;
        }
        Command::Hook { name, limit } => {
            let db = IndexDb::open(&db_path)?;
            let hits = db.find_by_kind(SymbolKind::Hook, &name, limit)?;
            print_records(&hits, &repo_dir, cli.json)?;
        }
        Command::Service { query, limit } => {
            let db = IndexDb::open(&db_path)?;
            let hits = db.find_by_kind(SymbolKind::Service, &query, limit)?;
            print_records(&hits, &repo_dir, cli.json)?;
        }
        Command::Route { query, limit } => {
            let db = IndexDb::open(&db_path)?;
            let hits = db.find_by_kind(SymbolKind::Route, &query, limit)?;
            print_records(&hits, &repo_dir, cli.json)?;
        }
        Command::Stats => {
            let db = IndexDb::open(&db_path)?;
            let stats = db.stats()?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&stats)
                        .map_err(|e| IndexError::InvalidArg(e.to_string()))?
                );
            } else {
                println!("commit: {}", stats.commit_sha);
                println!("indexed_at: {}", stats.indexed_at);
                println!("repo: {}", stats.repo_url);
                println!("files: {}", stats.total_files);
                println!("symbols: {}", stats.total_symbols);
                for item in &stats.by_kind {
                    println!("  {}: {}", item.kind, item.count);
                }
            }
        }
    }
    Ok(())
}

fn print_records(records: &[SymbolRecord], repo_dir: &std::path::Path, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(records)
                .map_err(|e| IndexError::InvalidArg(e.to_string()))?
        );
        return Ok(());
    }
    if records.is_empty() {
        println!("no matches");
        return Ok(());
    }
    for rec in records {
        let abs = repo_dir.join(&rec.file);
        println!(
            "[{}] {} ({}:{})",
            rec.kind,
            rec.fqn,
            abs.display(),
            rec.line
        );
        if !rec.signature.is_empty() && rec.signature != rec.fqn {
            println!("  sig: {}", rec.signature);
        }
        if !rec.attrs.is_empty() {
            println!("  attrs: {}", rec.attrs);
        }
        if !rec.doc.is_empty() {
            let short = truncate_chars(&rec.doc, 180);
            println!("  doc: {short}");
        }
    }
    Ok(())
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let mut chars = s.chars();
    let head: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}
