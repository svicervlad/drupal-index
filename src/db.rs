//! `SQLite` + FTS5 storage and query layer for `drupal-index`.

use crate::error::Result;
use crate::model::{IndexStats, KindCount, SymbolKind, SymbolRecord};
use rusqlite::{Connection, OptionalExtension, params};
use std::path::Path;

/// Handle to the local `SQLite` index database.
pub struct IndexDb {
    conn: Connection,
}

impl IndexDb {
    /// Opens (or creates) the `SQLite` database at `path` and ensures the schema exists.
    ///
    /// # Errors
    /// Returns [`crate::error::IndexError::Db`] if `SQLite` fails to open or initialize.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| crate::error::IndexError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA temp_store = MEMORY;

            CREATE TABLE IF NOT EXISTS meta (
                key   TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS symbols (
                id        INTEGER PRIMARY KEY,
                kind      TEXT NOT NULL,
                name      TEXT NOT NULL,
                fqn       TEXT NOT NULL,
                file      TEXT NOT NULL,
                line      INTEGER NOT NULL,
                end_line  INTEGER NOT NULL,
                signature TEXT NOT NULL,
                doc       TEXT NOT NULL,
                attrs     TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_symbols_name ON symbols(name);
            CREATE INDEX IF NOT EXISTS idx_symbols_fqn  ON symbols(fqn);
            CREATE INDEX IF NOT EXISTS idx_symbols_kind ON symbols(kind);

            CREATE VIRTUAL TABLE IF NOT EXISTS symbols_fts USING fts5(
                name,
                fqn,
                signature,
                doc,
                file,
                content='symbols',
                content_rowid='id',
                tokenize='unicode61 tokenchars ''_.\\:'''
            );
            ",
        )?;
        Ok(Self { conn })
    }

    /// Reads a metadata value by `key`.
    ///
    /// # Errors
    /// Returns [`crate::error::IndexError::Db`] on query failure.
    pub fn get_meta(&self, key: &str) -> Result<Option<String>> {
        let val = self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get::<_, String>(0)
            })
            .optional()?;
        Ok(val)
    }

    /// Replaces all indexed records in a single transaction and rebuilds the FTS5 index.
    ///
    /// # Errors
    /// Returns [`crate::error::IndexError::Db`] if any `SQLite` operation fails.
    pub fn replace_all(
        &mut self,
        commit_sha: &str,
        indexed_at: &str,
        repo_url: &str,
        records: &[SymbolRecord],
    ) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM symbols", [])?;

        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO symbols (kind, name, fqn, file, line, end_line, signature, doc, attrs)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )?;
            for rec in records {
                stmt.execute(params![
                    rec.kind.as_str(),
                    rec.name,
                    rec.fqn,
                    rec.file,
                    i64::try_from(rec.line).unwrap_or(i64::MAX),
                    i64::try_from(rec.end_line).unwrap_or(i64::MAX),
                    rec.signature,
                    rec.doc,
                    rec.attrs,
                ])?;
            }
        }

        tx.execute("INSERT INTO symbols_fts(symbols_fts) VALUES('rebuild')", [])?;
        set_meta_tx(&tx, "commit_sha", commit_sha)?;
        set_meta_tx(&tx, "indexed_at", indexed_at)?;
        set_meta_tx(&tx, "repo_url", repo_url)?;
        tx.commit()?;
        Ok(())
    }

    /// Searches indexed records using FTS5, optionally filtered by `kind`.
    ///
    /// # Errors
    /// Returns [`crate::error::IndexError::Db`] on query failure.
    pub fn search(
        &self,
        query: &str,
        kind: Option<SymbolKind>,
        limit: usize,
    ) -> Result<Vec<SymbolRecord>> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let fts_expr = build_fts_query(trimmed);
        let limit_i64 = i64::try_from(limit.min(500)).unwrap_or(500);

        let mut results = if let Some(k) = kind {
            let mut stmt = self.conn.prepare(
                "SELECT s.kind, s.name, s.fqn, s.file, s.line, s.end_line, s.signature, s.doc, s.attrs
                 FROM symbols_fts f
                 JOIN symbols s ON s.id = f.rowid
                 WHERE symbols_fts MATCH ?1 AND s.kind = ?2
                 ORDER BY bm25(symbols_fts, 10.0, 8.0, 4.0, 1.0, 2.0), s.id
                 LIMIT ?3",
            )?;
            collect_rows(stmt.query(params![fts_expr, k.as_str(), limit_i64])?)?
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT s.kind, s.name, s.fqn, s.file, s.line, s.end_line, s.signature, s.doc, s.attrs
                 FROM symbols_fts f
                 JOIN symbols s ON s.id = f.rowid
                 WHERE symbols_fts MATCH ?1
                 ORDER BY bm25(symbols_fts, 10.0, 8.0, 4.0, 1.0, 2.0), s.id
                 LIMIT ?2",
            )?;
            collect_rows(stmt.query(params![fts_expr, limit_i64])?)?
        };

        if results.is_empty() {
            let like = format!("%{trimmed}%");
            results = if let Some(k) = kind {
                let mut stmt = self.conn.prepare(
                    "SELECT kind, name, fqn, file, line, end_line, signature, doc, attrs
                     FROM symbols
                     WHERE kind = ?1 AND (name LIKE ?2 OR fqn LIKE ?2 OR signature LIKE ?2)
                     LIMIT ?3",
                )?;
                collect_rows(stmt.query(params![k.as_str(), like, limit_i64])?)?
            } else {
                let mut stmt = self.conn.prepare(
                    "SELECT kind, name, fqn, file, line, end_line, signature, doc, attrs
                     FROM symbols
                     WHERE name LIKE ?1 OR fqn LIKE ?1 OR signature LIKE ?1
                     LIMIT ?2",
                )?;
                collect_rows(stmt.query(params![like, limit_i64])?)?
            };
        }

        Ok(results)
    }

    /// Finds exact or suffix/FQN matches for a PHP symbol name.
    ///
    /// # Errors
    /// Returns [`crate::error::IndexError::Db`] on query failure.
    pub fn find_symbol(&self, name: &str, limit: usize) -> Result<Vec<SymbolRecord>> {
        let trimmed = name.trim().trim_start_matches('\\');
        let limit_i64 = i64::try_from(limit.min(500)).unwrap_or(500);

        let mut stmt = self.conn.prepare(
            "SELECT kind, name, fqn, file, line, end_line, signature, doc, attrs
             FROM symbols
             WHERE fqn = ?1 OR name = ?1
             ORDER BY CASE kind WHEN 'class' THEN 0 WHEN 'interface' THEN 1 WHEN 'trait' THEN 2 ELSE 3 END, id
             LIMIT ?2",
        )?;
        let exact = collect_rows(stmt.query(params![trimmed, limit_i64])?)?;
        if !exact.is_empty() {
            return Ok(exact);
        }

        let suffix = format!("%{trimmed}%");
        let mut stmt = self.conn.prepare(
            "SELECT kind, name, fqn, file, line, end_line, signature, doc, attrs
             FROM symbols
             WHERE fqn LIKE ?1 OR name LIKE ?1
             ORDER BY length(fqn), id
             LIMIT ?2",
        )?;
        collect_rows(stmt.query(params![suffix, limit_i64])?)
    }

    /// Finds records of a specific `kind` matching `query` by name, FQN, or FTS.
    ///
    /// # Errors
    /// Returns [`crate::error::IndexError::Db`] on query failure.
    pub fn find_by_kind(
        &self,
        kind: SymbolKind,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SymbolRecord>> {
        let trimmed = query.trim();
        let limit_i64 = i64::try_from(limit.min(500)).unwrap_or(500);
        let mut stmt = self.conn.prepare(
            "SELECT kind, name, fqn, file, line, end_line, signature, doc, attrs
             FROM symbols
             WHERE kind = ?1 AND (name = ?2 OR fqn = ?2)
             ORDER BY id
             LIMIT ?3",
        )?;
        let exact = collect_rows(stmt.query(params![kind.as_str(), trimmed, limit_i64])?)?;
        if !exact.is_empty() {
            return Ok(exact);
        }
        self.search(trimmed, Some(kind), limit)
    }

    /// Returns summary statistics for the current index.
    ///
    /// # Errors
    /// Returns [`crate::error::IndexError::Db`] on query failure.
    pub fn stats(&self) -> Result<IndexStats> {
        let commit_sha = self.get_meta("commit_sha")?.unwrap_or_default();
        let indexed_at = self.get_meta("indexed_at")?.unwrap_or_default();
        let repo_url = self.get_meta("repo_url")?.unwrap_or_default();

        let total_symbols: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get::<_, i64>(0))?
            .try_into()
            .unwrap_or(0);
        let total_files: usize = self
            .conn
            .query_row("SELECT COUNT(DISTINCT file) FROM symbols", [], |r| {
                r.get::<_, i64>(0)
            })?
            .try_into()
            .unwrap_or(0);

        let mut stmt = self
            .conn
            .prepare("SELECT kind, COUNT(*) FROM symbols GROUP BY kind ORDER BY COUNT(*) DESC")?;
        let mut rows = stmt.query([])?;
        let mut by_kind = Vec::new();
        while let Some(row) = rows.next()? {
            let kind: String = row.get(0)?;
            let count: usize = row.get::<_, i64>(1)?.try_into().unwrap_or(0);
            by_kind.push(KindCount { kind, count });
        }

        Ok(IndexStats {
            commit_sha,
            indexed_at,
            repo_url,
            total_symbols,
            total_files,
            by_kind,
        })
    }
}

fn set_meta_tx(tx: &rusqlite::Transaction<'_>, key: &str, value: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn collect_rows(mut rows: rusqlite::Rows<'_>) -> Result<Vec<SymbolRecord>> {
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let kind_str: String = row.get(0)?;
        let kind = SymbolKind::parse(&kind_str).unwrap_or(SymbolKind::Function);
        out.push(SymbolRecord {
            kind,
            name: row.get(1)?,
            fqn: row.get(2)?,
            file: row.get(3)?,
            line: row.get::<_, i64>(4)?.try_into().unwrap_or(1),
            end_line: row.get::<_, i64>(5)?.try_into().unwrap_or(1),
            signature: row.get(6)?,
            doc: row.get(7)?,
            attrs: row.get(8)?,
        });
    }
    Ok(out)
}

fn build_fts_query(raw: &str) -> String {
    raw.split_whitespace()
        .map(|tok| {
            let cleaned = tok.replace('"', "");
            format!("\"{cleaned}\"*")
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn stores_and_queries_symbols() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("index.sqlite");
        let mut db = IndexDb::open(&db_path).unwrap();
        let records = vec![SymbolRecord {
            kind: SymbolKind::Class,
            name: "EntityTypeManager".into(),
            fqn: r"Drupal\Core\Entity\EntityTypeManager".into(),
            file: "core/lib/Drupal/Core/Entity/EntityTypeManager.php".into(),
            line: 40,
            end_line: 300,
            signature: "class EntityTypeManager extends DefaultPluginManager".into(),
            doc: "Manages entity type plugin definitions.".into(),
            attrs: String::new(),
        }];
        db.replace_all("abc123", "1700000000", "https://example.com", &records)
            .unwrap();

        let hits = db.search("EntityTypeManager", None, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].line, 40);

        let stats = db.stats().unwrap();
        assert_eq!(stats.total_symbols, 1);
        assert_eq!(stats.commit_sha, "abc123");
    }
}
