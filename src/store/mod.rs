//! Persists logged requests to a local SQLite file and answers aggregate
//! usage queries against it.

mod query;
mod requests;

pub use query::{default_bucket, group_by_column, stats, time_series, StatRow, StoreError, TimeSeriesResult};
pub use requests::{
    distinct_models, distinct_session_ids, distinct_virtual_key_labels, get_request, list_requests, RequestFilter, RequestSummary,
};

use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS requests (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    ts                    TEXT NOT NULL,
    route                 TEXT NOT NULL,
    virtual_key_label     TEXT NOT NULL,
    real_key_label        TEXT NOT NULL,
    format                TEXT NOT NULL,
    model                 TEXT NOT NULL,
    path                  TEXT NOT NULL,
    stream                INTEGER NOT NULL,
    status_code           INTEGER NOT NULL,
    latency_ms            INTEGER NOT NULL,
    prompt_tokens         INTEGER NOT NULL,
    completion_tokens     INTEGER NOT NULL,
    total_tokens          INTEGER NOT NULL,
    cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens     INTEGER NOT NULL DEFAULT 0,
    cost_usd              REAL NOT NULL DEFAULT 0,
    request_body          TEXT,
    response_body         TEXT,
    error                 TEXT,
    virtual_key_value     TEXT NOT NULL DEFAULT '',
    session_id            TEXT NOT NULL DEFAULT '',
    request_headers       TEXT NOT NULL DEFAULT '',
    response_headers      TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_requests_ts ON requests(ts);
CREATE INDEX IF NOT EXISTS idx_requests_model ON requests(model);
CREATE INDEX IF NOT EXISTS idx_requests_route ON requests(route);
CREATE INDEX IF NOT EXISTS idx_requests_virtual_key_label ON requests(virtual_key_label);
CREATE INDEX IF NOT EXISTS idx_requests_real_key_label ON requests(real_key_label);
"#;

/// One logged proxy call. `id` is only populated by read paths
/// ([`get_request`]/[`list_requests`]) — [`Store::insert`] never sets or
/// reads it, since the column is an autoincrement primary key assigned
/// by SQLite.
#[derive(Debug, Clone, Serialize)]
pub struct Record {
    pub id: i64,
    pub timestamp: DateTime<Utc>,
    pub route: String,
    pub virtual_key_label: String,
    pub real_key_label: String,
    pub format: String,
    pub model: String,
    pub path: String,
    pub stream: bool,
    pub status_code: i64,
    pub latency_ms: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    pub cost_usd: f64,
    pub request_body: String,
    pub response_body: String,
    pub error: String,
    /// The virtual key value the client authenticated with — unlike the
    /// real upstream key, this is already plaintext-visible/copyable in
    /// the Routes UI, so storing it (to resolve the key's *current*
    /// label if renamed later) doesn't weaken the "real secrets never
    /// touch the log" guarantee below.
    pub virtual_key_value: String,
    /// Client-supplied `x-session-id` header, if any — lets the UI group
    /// requests that belong to the same conversation. Empty when the
    /// client didn't send one; there's no server-side heuristic fallback.
    pub session_id: String,
    /// Raw inbound request line + headers, captured before the real
    /// upstream key is injected — so it reflects what the client sent
    /// (their virtual key, not the real secret), never what was
    /// forwarded upstream.
    pub request_headers: String,
    /// Raw upstream status line + response headers (hop-by-hop headers
    /// already stripped).
    pub response_headers: String,
}

impl Default for Record {
    fn default() -> Self {
        Record {
            id: 0,
            timestamp: Utc::now(),
            route: String::new(),
            virtual_key_label: String::new(),
            real_key_label: String::new(),
            format: String::new(),
            model: String::new(),
            path: String::new(),
            stream: false,
            status_code: 0,
            latency_ms: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            cost_usd: 0.0,
            request_body: String::new(),
            response_body: String::new(),
            error: String::new(),
            virtual_key_value: String::new(),
            session_id: String::new(),
            request_headers: String::new(),
            response_headers: String::new(),
        }
    }
}

fn to_ts_string(ts: DateTime<Utc>) -> String {
    ts.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}

fn parse_ts_string(s: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| StoreError::Timestamp(s.to_string(), e))
}

fn open_connection(path: &str) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    Ok(conn)
}

/// Runs `SCHEMA` (a no-op `CREATE TABLE IF NOT EXISTS` on any database
/// that already has the `requests` table), then applies any columns
/// added after the table was first created via `ALTER TABLE ... ADD
/// COLUMN`, ignoring "duplicate column name" so this stays idempotent
/// across both fresh and pre-existing database files — SQLite has no
/// `ADD COLUMN IF NOT EXISTS`. Any index on a migrated column must be
/// created here too (after the column exists), not in `SCHEMA`'s
/// initial batch — `CREATE INDEX ... (session_id)` would otherwise
/// fail on a pre-existing database that hasn't been migrated yet.
fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)?;
    for stmt in [
        "ALTER TABLE requests ADD COLUMN virtual_key_value TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE requests ADD COLUMN session_id TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE requests ADD COLUMN request_headers TEXT NOT NULL DEFAULT ''",
        "ALTER TABLE requests ADD COLUMN response_headers TEXT NOT NULL DEFAULT ''",
    ] {
        match conn.execute(stmt, []) {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(_, Some(msg))) if msg.contains("duplicate column name") => {}
            Err(e) => return Err(e),
        }
    }
    conn.execute("CREATE INDEX IF NOT EXISTS idx_requests_session_id ON requests(session_id)", [])?;
    Ok(())
}

fn insert_record(conn: &Connection, r: &Record) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO requests (
            ts, route, virtual_key_label, real_key_label, format, model, path, stream, status_code, latency_ms,
            prompt_tokens, completion_tokens, total_tokens, cache_creation_tokens, cache_read_tokens, cost_usd,
            request_body, response_body, error, virtual_key_value, session_id, request_headers, response_headers
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        params![
            to_ts_string(r.timestamp),
            r.route,
            r.virtual_key_label,
            r.real_key_label,
            r.format,
            r.model,
            r.path,
            r.stream as i64,
            r.status_code,
            r.latency_ms,
            r.prompt_tokens,
            r.completion_tokens,
            r.total_tokens,
            r.cache_creation_tokens,
            r.cache_read_tokens,
            r.cost_usd,
            r.request_body,
            r.response_body,
            r.error,
            r.virtual_key_value,
            r.session_id,
            r.request_headers,
            r.response_headers,
        ],
    )?;
    Ok(())
}

fn write_loop(conn: Connection, rx: std::sync::mpsc::Receiver<Record>) {
    while let Ok(record) = rx.recv() {
        if let Err(e) = insert_record(&conn, &record) {
            tracing::error!("store: insert failed: {e}");
        }
    }
}

/// Owns a SQLite connection dedicated to writes, serializing all inserts
/// through a single background thread so concurrent proxy requests never
/// contend for the SQLite write lock, plus a separate connection for
/// reads (Stats/TimeSeries/request browsing), shared behind a mutex.
///
/// `write_tx`/`writer_handle` sit behind a `Mutex` (rather than being
/// plain fields only `Drop` touches) so [`Store::close`] can drain the
/// writer thread through a shared `&Store` — needed by callers like the
/// Tauri app that must flush pending writes *before* calling something
/// like `AppHandle::exit`, which unlike a normal `main()` return does not
/// run `Drop` on its way out.
pub struct Store {
    write_tx: Mutex<Option<SyncSender<Record>>>,
    writer_handle: Mutex<Option<JoinHandle<()>>>,
    read_conn: Arc<Mutex<Connection>>,
}

impl Store {
    /// Opens (creating if needed) the SQLite file at `path`, ensures the
    /// schema exists, and starts the background writer thread.
    pub fn open(path: &str) -> rusqlite::Result<Self> {
        let write_conn = open_connection(path)?;
        ensure_schema(&write_conn)?;
        let read_conn = open_connection(path)?;

        let (tx, rx) = sync_channel::<Record>(256);
        let writer_handle = thread::spawn(move || write_loop(write_conn, rx));

        Ok(Store {
            write_tx: Mutex::new(Some(tx)),
            writer_handle: Mutex::new(Some(writer_handle)),
            read_conn: Arc::new(Mutex::new(read_conn)),
        })
    }

    /// Queues a record to be written asynchronously; never blocks the
    /// caller on disk I/O (beyond a brief wait if the 256-deep queue is
    /// momentarily full).
    pub fn insert(&self, record: Record) {
        if let Some(tx) = self.write_tx.lock().expect("write_tx mutex poisoned").as_ref() {
            let _ = tx.send(record);
        }
    }

    /// A cloneable handle to the read connection, for Stats/TimeSeries/
    /// request-browsing queries run from any thread (e.g. inside
    /// `tokio::task::spawn_blocking`, since `rusqlite::Connection` calls
    /// are synchronous).
    pub fn read_conn(&self) -> Arc<Mutex<Connection>> {
        self.read_conn.clone()
    }

    /// Deletes all rows from the `requests` table. Runs synchronously on
    /// the read connection under its mutex. Any already-queued records in
    /// the writer thread may land after this returns, so callers that want
    /// a truly empty database should call [`Store::close`] first and
    /// reopen the store.
    pub fn clear(&self) -> rusqlite::Result<()> {
        let conn = self.read_conn.lock().expect("store read connection mutex poisoned");
        conn.execute("DELETE FROM requests", [])?;
        Ok(())
    }

    /// Closes the write channel and waits for the writer thread to drain
    /// every already-queued record. Idempotent — safe to call explicitly
    /// (e.g. before an app-initiated exit) and then again implicitly via
    /// [`Drop`], or from multiple call sites; only the first call does
    /// anything.
    pub fn close(&self) {
        self.write_tx.lock().expect("write_tx mutex poisoned").take();
        if let Some(handle) = self.writer_handle.lock().expect("writer_handle mutex poisoned").take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db_path(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("tollgate-store-test-{name}-{}.db", std::process::id()))
            .to_string_lossy()
            .to_string()
    }

    /// Simulates opening a database file created before `virtual_key_value`
    /// existed: builds the table by hand without that column, then
    /// confirms `ensure_schema`'s `ALTER TABLE` migration adds it without
    /// erroring, and running it a second time (e.g. on the next app
    /// launch) stays a no-op rather than failing on "duplicate column".
    #[test]
    fn ensure_schema_migrates_pre_existing_database_without_new_column() {
        let path = temp_db_path("migrate-virtual-key-value");
        {
            let conn = open_connection(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE requests (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    ts TEXT NOT NULL, route TEXT NOT NULL, virtual_key_label TEXT NOT NULL,
                    real_key_label TEXT NOT NULL, format TEXT NOT NULL, model TEXT NOT NULL,
                    path TEXT NOT NULL, stream INTEGER NOT NULL, status_code INTEGER NOT NULL,
                    latency_ms INTEGER NOT NULL, prompt_tokens INTEGER NOT NULL,
                    completion_tokens INTEGER NOT NULL, total_tokens INTEGER NOT NULL,
                    cache_creation_tokens INTEGER NOT NULL DEFAULT 0, cache_read_tokens INTEGER NOT NULL DEFAULT 0,
                    cost_usd REAL NOT NULL DEFAULT 0, request_body TEXT, response_body TEXT, error TEXT
                );",
            )
            .unwrap();
        }

        let conn = open_connection(&path).unwrap();
        ensure_schema(&conn).unwrap();
        ensure_schema(&conn).unwrap(); // second run must stay idempotent

        insert_record(&conn, &Record { route: "r1".to_string(), ..Default::default() }).unwrap();
        let value: String = conn.query_row("SELECT virtual_key_value FROM requests LIMIT 1", [], |r| r.get(0)).unwrap();
        assert_eq!(value, "");
        let (req_headers, resp_headers): (String, String) = conn
            .query_row("SELECT request_headers, response_headers FROM requests LIMIT 1", [], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap();
        assert_eq!(req_headers, "");
        assert_eq!(resp_headers, "");

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn clear_removes_all_rows() {
        let path = temp_db_path("clear-removes-all-rows");
        let store = Store::open(&path).unwrap();
        store.insert(Record { route: "r1".to_string(), ..Default::default() });
        store.insert(Record { route: "r2".to_string(), ..Default::default() });
        drop(store); // wait for the writer thread to drain

        let store = Store::open(&path).unwrap();
        store.clear().unwrap();
        drop(store);

        let conn = Connection::open(&path).unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM requests", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 0);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }

    #[test]
    fn insert_and_read_back() {
        let path = temp_db_path("insert-and-read-back");
        let store = Store::open(&path).unwrap();
        store.insert(Record {
            route: "r1".to_string(),
            model: "gpt-4o".to_string(),
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            cost_usd: 0.001,
            ..Default::default()
        });
        drop(store); // wait for the writer thread to drain

        let conn = Connection::open(&path).unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM requests", [], |r| r.get(0)).unwrap();
        assert_eq!(count, 1);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{path}-wal"));
        let _ = std::fs::remove_file(format!("{path}-shm"));
    }
}
