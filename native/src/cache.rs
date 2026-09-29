//! Persistent index cache (SQLite). One row per document keyed by the SHA-256
//! of the PDF bytes; chunk embeddings stored as little-endian f32 BLOBs.

use crate::chunker::Chunk;
use crate::search::Matrix;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// On-disk layout version (SQLite `PRAGMA user_version`). Bump it whenever the
/// tables change incompatibly: databases with any other non-zero version are
/// wiped and recreated (the cache is regenerable), so an old layout can never be
/// misread. Version 0 = database created by LegalSearch 1.0.0, whose layout is
/// identical to version 1 and is adopted as is.
///
/// Note: the BM25 index (hybrid retrieval) is NOT stored; it is rebuilt in
/// memory from `chunks.text`, so it needs no on-disk format change.
pub const SCHEMA_VERSION: i64 = 1;

/// Least-recently-used documents beyond this count are evicted to bound disk use.
pub const MAX_DOCUMENTS: i64 = 300;

pub struct Cache {
    conn: Connection,
}

#[derive(Debug, Clone)]
pub struct ChunkMeta {
    pub page: u32,
    pub chunk_index: u32,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct LoadedIndex {
    pub sha256: String,
    pub chunks: Vec<ChunkMeta>,
    pub matrix: Matrix,
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn f32s_to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn blob_to_f32s(b: &[u8]) -> Option<Vec<f32>> {
    if b.len() % 4 != 0 {
        return None;
    }
    Some(b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect())
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS documents (
    id            INTEGER PRIMARY KEY,
    sha256        TEXT NOT NULL UNIQUE,
    filename      TEXT NOT NULL,
    page_count    INTEGER NOT NULL,
    chunk_count   INTEGER NOT NULL,
    model_version TEXT NOT NULL,
    dim           INTEGER NOT NULL,
    indexed_at    INTEGER NOT NULL,
    last_used_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS chunks (
    document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    chunk_index INTEGER NOT NULL,
    page        INTEGER NOT NULL,
    text        TEXT NOT NULL,
    embedding   BLOB NOT NULL,
    PRIMARY KEY (document_id, chunk_index)
);
";

impl Cache {
    pub fn open(path: &Path) -> rusqlite::Result<Cache> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> rusqlite::Result<Cache> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Cache> {
        conn.busy_timeout(std::time::Duration::from_secs(10))?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != 0 && version != SCHEMA_VERSION {
            crate::info!("cache schema v{version} != v{SCHEMA_VERSION}: rebuilding cache");
            conn.execute_batch("DROP TABLE IF EXISTS chunks; DROP TABLE IF EXISTS documents;")?;
        }
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
        Ok(Cache { conn })
    }

    /// Returns chunk count if the document is indexed with the current model.
    pub fn lookup(&self, sha256: &str, model_version: &str) -> rusqlite::Result<Option<u32>> {
        self.conn
            .query_row(
                "SELECT chunk_count FROM documents WHERE sha256 = ?1 AND model_version = ?2",
                params![sha256, model_version],
                |r| r.get::<_, u32>(0),
            )
            .optional()
    }

    pub fn load(&self, sha256: &str, model_version: &str) -> rusqlite::Result<Option<LoadedIndex>> {
        let doc: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT id, dim FROM documents WHERE sha256 = ?1 AND model_version = ?2",
                params![sha256, model_version],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let Some((doc_id, dim)) = doc else { return Ok(None) };
        let dim = dim as usize;
        let mut stmt = self.conn.prepare(
            "SELECT page, chunk_index, text, embedding FROM chunks WHERE document_id = ?1 ORDER BY chunk_index",
        )?;
        let mut chunks = Vec::new();
        let mut matrix = Matrix::new(dim);
        let mut rows = stmt.query(params![doc_id])?;
        while let Some(r) = rows.next()? {
            let blob: Vec<u8> = r.get(3)?;
            let Some(v) = blob_to_f32s(&blob).filter(|v| v.len() == dim) else {
                // Corrupted row: treat the whole document as not cached.
                drop(rows);
                drop(stmt);
                self.remove(sha256)?;
                return Ok(None);
            };
            chunks.push(ChunkMeta { page: r.get(0)?, chunk_index: r.get(1)?, text: r.get(2)? });
            matrix.push(&v);
        }
        self.conn.execute("UPDATE documents SET last_used_at = ?1 WHERE id = ?2", params![now(), doc_id])?;
        Ok(Some(LoadedIndex { sha256: sha256.to_string(), chunks, matrix }))
    }

    pub fn store(
        &mut self,
        sha256: &str,
        filename: &str,
        page_count: u32,
        model_version: &str,
        chunks: &[Chunk],
        embeddings: &Matrix,
    ) -> rusqlite::Result<()> {
        assert_eq!(chunks.len(), embeddings.rows());
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM documents WHERE sha256 = ?1", params![sha256])?;
        let t = now();
        tx.execute(
            "INSERT INTO documents (sha256, filename, page_count, chunk_count, model_version, dim, indexed_at, last_used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![sha256, filename, page_count, chunks.len() as i64, model_version, embeddings.dim as i64, t],
        )?;
        let doc_id = tx.last_insert_rowid();
        {
            let mut ins = tx.prepare(
                "INSERT INTO chunks (document_id, chunk_index, page, text, embedding) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for (i, c) in chunks.iter().enumerate() {
                ins.execute(params![doc_id, c.chunk_index, c.page, c.text, f32s_to_blob(embeddings.row(i))])?;
            }
        }
        tx.execute(
            "DELETE FROM documents WHERE id NOT IN (SELECT id FROM documents ORDER BY last_used_at DESC, id DESC LIMIT ?1)",
            params![MAX_DOCUMENTS],
        )?;
        tx.commit()
    }

    pub fn remove(&self, sha256: &str) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM documents WHERE sha256 = ?1", params![sha256])?;
        Ok(())
    }

    pub fn document_count(&self) -> rusqlite::Result<i64> {
        self.conn.query_row("SELECT COUNT(*) FROM documents", [], |r| r.get(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn sample() -> (Vec<Chunk>, Matrix) {
        let chunks = vec![
            Chunk { page: 1, chunk_index: 0, text: "uno".into() },
            Chunk { page: 3, chunk_index: 1, text: "tres ñandú".into() },
        ];
        let mut m = Matrix::new(3);
        m.push(&[1.0, 0.0, 0.0]);
        m.push(&[0.0, 0.6, 0.8]);
        (chunks, m)
    }

    #[test]
    fn miss_then_hit() {
        let mut c = Cache::open_in_memory().unwrap();
        assert_eq!(c.lookup(A, "m1").unwrap(), None);
        assert!(c.load(A, "m1").unwrap().is_none());
        let (ch, m) = sample();
        c.store(A, "a.pdf", 3, "m1", &ch, &m).unwrap();
        assert_eq!(c.lookup(A, "m1").unwrap(), Some(2));
        let idx = c.load(A, "m1").unwrap().unwrap();
        assert_eq!(idx.chunks.len(), 2);
        assert_eq!(idx.chunks[1].page, 3);
        assert_eq!(idx.chunks[1].text, "tres ñandú");
        assert_eq!(idx.matrix.row(1), &[0.0, 0.6, 0.8]);
        // Other docs are still misses.
        assert_eq!(c.lookup(B, "m1").unwrap(), None);
    }

    #[test]
    fn model_change_invalidates() {
        let mut c = Cache::open_in_memory().unwrap();
        let (ch, m) = sample();
        c.store(A, "a.pdf", 3, "m1", &ch, &m).unwrap();
        assert_eq!(c.lookup(A, "m2").unwrap(), None);
    }

    #[test]
    fn restore_replaces() {
        let mut c = Cache::open_in_memory().unwrap();
        let (ch, m) = sample();
        c.store(A, "a.pdf", 3, "m1", &ch, &m).unwrap();
        c.store(A, "a.pdf", 3, "m1", &ch[..1], &{
            let mut m1 = Matrix::new(3);
            m1.push(m.row(0));
            m1
        })
        .unwrap();
        assert_eq!(c.lookup(A, "m1").unwrap(), Some(1));
        assert_eq!(c.document_count().unwrap(), 1);
    }

    #[test]
    fn persists_across_connections() {
        let dir = std::env::temp_dir().join(format!("ls-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("index.sqlite");
        let (ch, m) = sample();
        {
            let mut c = Cache::open(&db).unwrap();
            c.store(B, "b.pdf", 3, "m1", &ch, &m).unwrap();
        }
        {
            let c = Cache::open(&db).unwrap();
            assert!(c.load(B, "m1").unwrap().is_some());
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    fn temp_db(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("ls-cache-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("index.sqlite");
        let _ = std::fs::remove_file(&db);
        db
    }

    #[test]
    fn schema_version_is_stamped() {
        let c = Cache::open_in_memory().unwrap();
        let v: i64 = c.conn.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(v, SCHEMA_VERSION);
    }

    #[test]
    fn incompatible_schema_is_invalidated_cleanly() {
        let db = temp_db("schema");
        let (ch, m) = sample();
        {
            let mut c = Cache::open(&db).unwrap();
            c.store(A, "a.pdf", 3, "m1", &ch, &m).unwrap();
            // simulate a database written by a future/incompatible version
            c.conn.execute_batch("PRAGMA user_version = 99;").unwrap();
        }
        let c = Cache::open(&db).unwrap();
        assert_eq!(c.lookup(A, "m1").unwrap(), None, "old layout must never be read");
        assert_eq!(c.document_count().unwrap(), 0);
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    #[test]
    fn legacy_v1_0_0_database_is_kept() {
        // 1.0.0 databases have user_version 0 and the same layout: still valid,
        // and hybrid retrieval rebuilds BM25 from their stored chunk texts.
        let db = temp_db("legacy");
        let (ch, m) = sample();
        {
            let mut c = Cache::open(&db).unwrap();
            c.store(B, "b.pdf", 3, "m1", &ch, &m).unwrap();
            c.conn.execute_batch("PRAGMA user_version = 0;").unwrap();
        }
        let c = Cache::open(&db).unwrap();
        assert_eq!(c.lookup(B, "m1").unwrap(), Some(2));
        std::fs::remove_dir_all(db.parent().unwrap()).ok();
    }

    #[test]
    fn corrupted_blob_is_a_miss() {
        let mut c = Cache::open_in_memory().unwrap();
        let (ch, m) = sample();
        c.store(A, "a.pdf", 3, "m1", &ch, &m).unwrap();
        c.conn.execute("UPDATE chunks SET embedding = x'0102' WHERE chunk_index = 1", []).unwrap();
        assert!(c.load(A, "m1").unwrap().is_none());
        assert_eq!(c.lookup(A, "m1").unwrap(), None);
    }
}
