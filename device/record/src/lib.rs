//! The record: one SQLite database per process, one row per event, one sequence.
//!
//! **ONE FILE, ONE WRITER, ONE TRANSACTION PER EVENT.** A row is `(seq, ts_ms, json)`; a publish
//! inserts it and prunes the oldest rows under a byte budget in the same transaction. A reader
//! asks by sequence or by moment; the record never guesses a row inline. WAL plus
//! `synchronous=FULL` is what makes a committed row survive a crash and a half-written one
//! impossible.
//!
//! The bus is this crate's first user: envelopes are JSON rows, and the change feed is the `seq`
//! column. A second process (the console's operator record) opens its own file with the same
//! interface; nothing here knows what a row means.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension};

/// The name of the database inside the directory a record is opened on.
pub const FILE: &str = "record.db";

/// SQLite errors become the `io::Result` this API already returns.
trait Db<T> {
    fn db(self) -> io::Result<T>;
}

impl<T> Db<T> for rusqlite::Result<T> {
    fn db(self) -> io::Result<T> {
        self.map_err(io::Error::other)
    }
}

/// One recorded event: the sequence it was given, its moment, and its JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub seq: u64,
    pub ts_ms: i64,
    pub json: String,
}

/// One record's database and its sequence.
pub struct Record {
    dir: PathBuf,
    conn: Mutex<Connection>,
    max_bytes: u64,
    head: AtomicU64,
}

impl Record {
    /// Open (or create) the record in `dir`, at `dir/record.db`, with a byte budget.
    pub fn open(dir: impl AsRef<Path>, max_bytes: u64) -> io::Result<Self> {
        Self::open_file(dir.as_ref().join(FILE), max_bytes)
    }

    /// Open (or create) the record at an explicit file path.
    pub fn open_file(path: impl AsRef<Path>, max_bytes: u64) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        std::fs::create_dir_all(&dir)?;
        let conn = Connection::open(&path).db()?;
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = FULL;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS feed (
                 seq    INTEGER PRIMARY KEY,
                 ts_ms  INTEGER NOT NULL,
                 json   TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             INSERT OR IGNORE INTO meta (key, value) VALUES ('schema_version', '1');
             INSERT OR IGNORE INTO meta (key, value)
                 SELECT 'bytes', CAST(COALESCE(SUM(LENGTH(CAST(json AS BLOB))), 0) AS TEXT) FROM feed;
             INSERT OR IGNORE INTO meta (key, value)
                 SELECT 'rows', CAST(COUNT(*) AS TEXT) FROM feed;",
        )
        .db()?;
        let last: i64 = conn
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM feed", [], |row| {
                row.get(0)
            })
            .db()?;
        Ok(Self {
            dir,
            conn: Mutex::new(conn),
            max_bytes,
            head: AtomicU64::new(last.max(0) as u64),
        })
    }

    /// The directory the record lives in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The last sequence written. Zero when the record is empty.
    pub fn last_seq(&self) -> u64 {
        self.head.load(Ordering::SeqCst)
    }

    /// The first sequence still retained. Zero when the record is empty.
    pub fn oldest_seq(&self) -> u64 {
        let conn = self.lock();
        conn.query_row("SELECT MIN(seq) FROM feed", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .optional()
        .ok()
        .flatten()
        .flatten()
        .map(|seq| seq.max(0) as u64)
        .unwrap_or(0)
    }

    /// Append one row and return the sequence it was given.
    pub fn append(&self, ts_ms: i64, json: String) -> io::Result<u64> {
        let mut conn = self.lock();
        let transaction = conn.transaction().db()?;
        // The sequence is read inside the transaction, not from this handle's memory: a second
        // connection (a test seeding a record, a tool reading it later) must not mint a sequence
        // the record has already used.
        let seq: u64 = transaction
            .query_row("SELECT COALESCE(MAX(seq), 0) + 1 FROM feed", [], |row| {
                row.get::<_, i64>(0)
            })
            .db()? as u64;
        let bytes = json.len() as i64;
        transaction
            .execute(
                "INSERT INTO feed (seq, ts_ms, json) VALUES (?1, ?2, ?3)",
                params![seq as i64, ts_ms, json],
            )
            .db()?;
        bump(&transaction, bytes, 1)?;
        prune(&transaction, self.max_bytes)?;
        transaction.commit().db()?;
        self.head.store(seq, Ordering::SeqCst);
        Ok(seq)
    }

    /// Append many rows in one transaction: the seed/import path, where one commit per row would
    /// pay an fsync per row and a retention check per row.
    pub fn append_many(&self, entries: &[(i64, String)]) -> io::Result<u64> {
        if entries.is_empty() {
            return Ok(self.head.load(Ordering::SeqCst));
        }
        let mut conn = self.lock();
        let transaction = conn.transaction().db()?;
        let mut seq: u64 = transaction
            .query_row("SELECT COALESCE(MAX(seq), 0) FROM feed", [], |row| {
                row.get::<_, i64>(0)
            })
            .db()? as u64;
        let mut bytes = 0i64;
        for (ts_ms, json) in entries {
            seq += 1;
            bytes += json.len() as i64;
            transaction
                .execute(
                    "INSERT INTO feed (seq, ts_ms, json) VALUES (?1, ?2, ?3)",
                    params![seq as i64, *ts_ms, json],
                )
                .db()?;
        }
        bump(&transaction, bytes, entries.len() as i64)?;
        prune(&transaction, self.max_bytes)?;
        transaction.commit().db()?;
        self.head.store(seq, Ordering::SeqCst);
        Ok(seq)
    }

    /// Insert a row with the sequence it already carries (the import path); an existing sequence is
    /// left alone.
    pub fn insert(&self, row: &Row) -> io::Result<()> {
        let mut conn = self.lock();
        let transaction = conn.transaction().db()?;
        let inserted = transaction
            .execute(
                "INSERT OR IGNORE INTO feed (seq, ts_ms, json) VALUES (?1, ?2, ?3)",
                params![row.seq as i64, row.ts_ms, &row.json],
            )
            .db()?;
        if inserted > 0 {
            bump(&transaction, row.json.len() as i64, 1)?;
        }
        prune(&transaction, self.max_bytes)?;
        transaction.commit().db()?;
        self.head.fetch_max(row.seq, Ordering::SeqCst);
        Ok(())
    }

    /// Delete every row (a test seed replacing a record; there is no production caller).
    pub fn clear(&self) -> io::Result<()> {
        let mut conn = self.lock();
        let transaction = conn.transaction().db()?;
        transaction.execute("DELETE FROM feed", []).db()?;
        set_counter(&transaction, "bytes", 0)?;
        set_counter(&transaction, "rows", 0)?;
        transaction.commit().db()?;
        self.head.store(0, Ordering::SeqCst);
        Ok(())
    }

    /// The newest `limit` rows, newest first.
    pub fn newest(&self, limit: usize) -> io::Result<Vec<Row>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        let conn = self.lock();
        let mut statement = conn
            .prepare("SELECT seq, ts_ms, json FROM feed ORDER BY seq DESC LIMIT ?1")
            .db()?;
        let rows = statement.query_map(params![limit as i64], row_of).db()?;
        collect(rows)
    }

    /// Every retained row from `from` inclusive, oldest first (the subscriber's replay).
    pub fn read_from(&self, from: u64) -> io::Result<Vec<Row>> {
        let conn = self.lock();
        let mut statement = conn
            .prepare("SELECT seq, ts_ms, json FROM feed WHERE seq >= ?1 ORDER BY seq ASC")
            .db()?;
        let rows = statement.query_map(params![from as i64], row_of).db()?;
        collect(rows)
    }

    /// A page, newest first: rows older than `before` (all when `None`), at most `limit`, and
    /// whether a row lies below the page.
    pub fn window(&self, before: Option<u64>, limit: usize) -> io::Result<(Vec<Row>, bool)> {
        if limit == 0 {
            return Ok((Vec::new(), false));
        }
        let conn = self.lock();
        let mut statement = match before {
            Some(before) => {
                let mut statement = conn
                    .prepare(
                        "SELECT seq, ts_ms, json FROM feed WHERE seq < ?1 ORDER BY seq DESC LIMIT ?2",
                    )
                    .db()?;
                let rows = statement
                    .query_map(params![before as i64, (limit + 1) as i64], row_of)
                    .db()?;
                collect(rows)?
            }
            None => {
                let mut statement = conn
                    .prepare("SELECT seq, ts_ms, json FROM feed ORDER BY seq DESC LIMIT ?1")
                    .db()?;
                let rows = statement
                    .query_map(params![(limit + 1) as i64], row_of)
                    .db()?;
                collect(rows)?
            }
        };
        let more = statement.len() > limit;
        statement.truncate(limit);
        Ok((statement, more))
    }

    /// Rows whose moment falls in `[from_ms, to_ms)`, newest first, at most `limit`, and whether
    /// the range held more.
    pub fn between(&self, from_ms: u64, to_ms: u64, limit: usize) -> io::Result<(Vec<Row>, bool)> {
        if limit == 0 || to_ms <= from_ms {
            return Ok((Vec::new(), false));
        }
        let conn = self.lock();
        let mut statement = conn
            .prepare(
                "SELECT seq, ts_ms, json FROM feed WHERE ts_ms >= ?1 AND ts_ms < ?2 \
                 ORDER BY seq DESC LIMIT ?3",
            )
            .db()?;
        let rows = statement
            .query_map(
                params![from_ms as i64, to_ms as i64, (limit + 1) as i64],
                row_of,
            )
            .db()?;
        let mut rows = collect(rows)?;
        let more = rows.len() > limit;
        rows.truncate(limit);
        Ok((rows, more))
    }

    /// The oldest and newest moments the record retains, each when it has a row.
    pub fn span(&self) -> io::Result<(Option<i64>, Option<i64>)> {
        let conn = self.lock();
        let (oldest, newest): (Option<i64>, Option<i64>) = conn
            .query_row("SELECT MIN(ts_ms), MAX(ts_ms) FROM feed", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .db()?;
        Ok((oldest, newest))
    }

    /// One meta value.
    pub fn meta(&self, key: &str) -> io::Result<Option<String>> {
        let conn = self.lock();
        conn.query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()
        .db()
    }

    /// Set one meta value.
    pub fn set_meta(&self, key: &str, value: &str) -> io::Result<()> {
        let conn = self.lock();
        conn.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )
        .db()?;
        Ok(())
    }

    /// Copy the record to `path` (`VACUUM INTO`).
    pub fn backup_to(&self, path: impl AsRef<Path>) -> io::Result<()> {
        let conn = self.lock();
        conn.execute("VACUUM INTO ?1", params![path.as_ref().to_string_lossy()])
            .db()?;
        Ok(())
    }

    /// `PRAGMA integrity_check`: `Ok` only when SQLite answers `ok`.
    pub fn check(&self) -> io::Result<()> {
        let conn = self.lock();
        let answer: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .db()?;
        if answer == "ok" {
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::InvalidData, answer))
        }
    }

    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }
}

fn row_of(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        seq: row.get::<_, i64>(0)? as u64,
        ts_ms: row.get(1)?,
        json: row.get(2)?,
    })
}

fn collect<F>(rows: rusqlite::MappedRows<'_, F>) -> io::Result<Vec<Row>>
where
    F: FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<Row>,
{
    let mut rows_out = Vec::new();
    for row in rows {
        rows_out.push(row.map_err(io::Error::other)?);
    }
    Ok(rows_out)
}

/// Delete the oldest rows while the stored bytes exceed `max_bytes`, always leaving the newest.
fn counter(transaction: &rusqlite::Transaction<'_>, key: &str) -> io::Result<i64> {
    transaction
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .db()?
        .parse::<i64>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn set_counter(transaction: &rusqlite::Transaction<'_>, key: &str, value: i64) -> io::Result<()> {
    transaction
        .execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value.to_string()],
        )
        .db()?;
    Ok(())
}

fn bump(transaction: &rusqlite::Transaction<'_>, bytes: i64, rows: i64) -> io::Result<()> {
    set_counter(transaction, "bytes", counter(transaction, "bytes")? + bytes)?;
    set_counter(transaction, "rows", counter(transaction, "rows")? + rows)
}

/// Delete the oldest rows while the stored bytes exceed `max_bytes`, always leaving the newest.
/// The counters live in `meta` and move in the same transaction, so a publish costs one insert,
/// one counter update and at most a few deletes — not a full-table scan.
fn prune(transaction: &rusqlite::Transaction<'_>, max_bytes: u64) -> io::Result<()> {
    loop {
        let rows = counter(transaction, "rows")?;
        if rows <= 1 {
            return Ok(());
        }
        let bytes = counter(transaction, "bytes")?;
        if (bytes as u64) <= max_bytes {
            return Ok(());
        }
        let removed: i64 = transaction
            .query_row(
                "SELECT LENGTH(CAST(json AS BLOB)) FROM feed ORDER BY seq ASC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .db()?;
        transaction
            .execute(
                "DELETE FROM feed WHERE seq = (SELECT MIN(seq) FROM feed)",
                [],
            )
            .db()?;
        bump(transaction, -removed, -1)?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "summrise-record-{}-{}-{}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn row(seq: u64, ts_ms: i64, json: &str) -> Row {
        Row {
            seq,
            ts_ms,
            json: json.to_owned(),
        }
    }

    #[test]
    fn append_assigns_the_sequence_and_reopening_continues_it() {
        let dir = temp_dir("sequence");
        {
            let record = Record::open(&dir, 1_000_000).unwrap();
            assert_eq!(record.append(10, r#"{"i":1}"#.to_owned()).unwrap(), 1);
            assert_eq!(record.append(20, r#"{"i":2}"#.to_owned()).unwrap(), 2);
        }
        let record = Record::open(&dir, 1_000_000).unwrap();
        assert_eq!(record.last_seq(), 2);
        assert_eq!(record.append(30, r#"{"i":3}"#.to_owned()).unwrap(), 3);
    }

    #[test]
    fn newest_and_window_read_newest_first_with_more() {
        let dir = temp_dir("window");
        let record = Record::open(&dir, 1_000_000).unwrap();
        for index in 0..5 {
            record.append(index, format!(r#"{{"i":{index}}}"#)).unwrap();
        }
        let newest = record.newest(3).unwrap();
        assert_eq!(
            newest.iter().map(|row| row.seq).collect::<Vec<_>>(),
            [5, 4, 3]
        );
        let (page, more) = record.window(None, 2).unwrap();
        assert_eq!(page.iter().map(|row| row.seq).collect::<Vec<_>>(), [5, 4]);
        assert!(more);
        let (page, more) = record.window(Some(2), 2).unwrap();
        assert_eq!(page.iter().map(|row| row.seq).collect::<Vec<_>>(), [1]);
        assert!(!more);
        assert!(record.newest(0).unwrap().is_empty());
    }

    #[test]
    fn between_reads_by_the_rows_own_moment() {
        let dir = temp_dir("between");
        let record = Record::open(&dir, 1_000_000).unwrap();
        record.append(10, r#"{"i":1}"#.to_owned()).unwrap();
        record.append(20, r#"{"i":2}"#.to_owned()).unwrap();
        record.append(30, r#"{"i":3}"#.to_owned()).unwrap();
        let (rows, more) = record.between(10, 30, 10).unwrap();
        assert_eq!(rows.iter().map(|row| row.seq).collect::<Vec<_>>(), [2, 1]);
        assert!(!more);
        assert!(record.between(10, 10, 10).unwrap().0.is_empty());
    }

    #[test]
    fn retention_deletes_the_oldest_rows_and_keeps_the_newest() {
        let dir = temp_dir("retention");
        let record = Record::open(&dir, 400).unwrap();
        for index in 0..10 {
            record
                .append(
                    index,
                    format!(r#"{{"i":{index},"pad":"{}"}}"#, "x".repeat(40)),
                )
                .unwrap();
        }
        assert_eq!(record.last_seq(), 10);
        assert!(record.oldest_seq() > 1);
        assert!(record.oldest_seq() < 10);
    }

    #[test]
    fn insert_keeps_an_existing_sequence_and_ignores_a_duplicate() {
        let dir = temp_dir("insert");
        let record = Record::open(&dir, 1_000_000).unwrap();
        record.insert(&row(5, 50, r#"{"i":5}"#)).unwrap();
        record.insert(&row(5, 50, r#"{"i":5}"#)).unwrap();
        assert_eq!(record.last_seq(), 5);
        assert_eq!(record.newest(10).unwrap().len(), 1);
    }

    #[test]
    fn meta_is_written_once_and_read_back() {
        let dir = temp_dir("meta");
        let record = Record::open(&dir, 1_000_000).unwrap();
        assert!(record.meta("import").unwrap().is_none());
        record.set_meta("import", "done").unwrap();
        record.set_meta("import", "done-again").unwrap();
        assert_eq!(
            record.meta("import").unwrap().as_deref(),
            Some("done-again")
        );
    }

    #[test]
    fn a_backup_is_a_readable_record() {
        let dir = temp_dir("backup");
        let record = Record::open(&dir, 1_000_000).unwrap();
        record.append(10, r#"{"i":1}"#.to_owned()).unwrap();
        let copy_dir = temp_dir("backup-copy");
        record.backup_to(copy_dir.join(FILE)).unwrap();
        let copy = Record::open(&copy_dir, 1_000_000).unwrap();
        assert_eq!(copy.last_seq(), 1);
        assert_eq!(copy.newest(1).unwrap()[0].json, r#"{"i":1}"#);
    }

    #[test]
    fn check_answers_ok() {
        let dir = temp_dir("check");
        let record = Record::open(&dir, 1_000_000).unwrap();
        record.append(10, r#"{"i":1}"#.to_owned()).unwrap();
        record.check().expect("a fresh record is intact");
    }
}
