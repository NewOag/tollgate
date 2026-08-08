use chrono::{DateTime, Utc};
use rusqlite::{params_from_iter, Connection, Row};
use serde::Serialize;

use super::{parse_ts_string, to_ts_string, Record, StoreError};

/// One row in the request browser list view — enough to render a table
/// without pulling full request/response body content over the wire
/// (see [`get_request`] for that).
#[derive(Debug, Clone, Serialize)]
pub struct RequestSummary {
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
    pub cost_usd: f64,
    pub error: String,
}

/// Narrows [`list_requests`] to matching rows. `None` fields mean "any";
/// `since` is always applied when set.
#[derive(Debug, Clone, Default)]
pub struct RequestFilter {
    pub since: Option<DateTime<Utc>>,
    pub until: Option<DateTime<Utc>>,
    pub route: Option<String>,
    pub model: Option<String>,
    pub virtual_key_label: Option<String>,
    pub real_key_label: Option<String>,
    pub session_id: Option<String>,
}

/// Turns `f` into a "WHERE ..." clause plus its positional string args,
/// applying only the conditions with a value set so callers don't have
/// to special-case which filters are actually in use. `LIMIT`/`OFFSET`
/// are handled separately by the caller since they're plain validated
/// integers, not filter values.
fn build_filter_where(f: &RequestFilter) -> (String, Vec<String>) {
    let mut clause = String::from("1=1");
    let mut args = Vec::new();
    if let Some(since) = f.since {
        clause += " AND ts >= ?";
        args.push(to_ts_string(since));
    }
    if let Some(until) = f.until {
        clause += " AND ts <= ?";
        args.push(to_ts_string(until));
    }
    if let Some(route) = &f.route {
        clause += " AND route = ?";
        args.push(route.clone());
    }
    if let Some(model) = &f.model {
        clause += " AND model = ?";
        args.push(model.clone());
    }
    if let Some(vk) = &f.virtual_key_label {
        clause += " AND virtual_key_label = ?";
        args.push(vk.clone());
    }
    if let Some(rk) = &f.real_key_label {
        clause += " AND real_key_label = ?";
        args.push(rk.clone());
    }
    if let Some(sid) = &f.session_id {
        clause += " AND session_id = ?";
        args.push(sid.clone());
    }
    (clause, args)
}

fn row_to_summary(row: &Row) -> Result<RequestSummary, StoreError> {
    let ts_str: String = row.get(1)?;
    let stream: i64 = row.get(8)?;
    Ok(RequestSummary {
        id: row.get(0)?,
        timestamp: parse_ts_string(&ts_str)?,
        route: row.get(2)?,
        virtual_key_label: row.get(3)?,
        real_key_label: row.get(4)?,
        format: row.get(5)?,
        model: row.get(6)?,
        path: row.get(7)?,
        stream: stream != 0,
        status_code: row.get(9)?,
        latency_ms: row.get(10)?,
        prompt_tokens: row.get(11)?,
        completion_tokens: row.get(12)?,
        total_tokens: row.get(13)?,
        cost_usd: row.get(14)?,
        error: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
    })
}

/// Returns the most recent requests matching `f` (newest first), limited
/// and offset for pagination (`limit` is clamped to `[1, 200]`), plus the
/// total count matching `f` (ignoring limit/offset) so callers can
/// render "page N of M" without a second round-trip of their own.
pub fn list_requests(
    conn: &Connection,
    f: &RequestFilter,
    limit: i64,
    offset: i64,
) -> Result<(Vec<RequestSummary>, i64), StoreError> {
    let (where_clause, args) = build_filter_where(f);
    let limit = limit.clamp(1, 200);
    let offset = offset.max(0);

    let count_query = format!("SELECT COUNT(*) FROM requests WHERE {where_clause}");
    let total: i64 = conn.query_row(&count_query, params_from_iter(args.iter()), |row| row.get(0))?;

    let list_query = format!(
        "SELECT id, ts, route, virtual_key_label, real_key_label, format, model, path, stream,
            status_code, latency_ms, prompt_tokens, completion_tokens, total_tokens, cost_usd, error
        FROM requests
        WHERE {where_clause}
        ORDER BY id DESC
        LIMIT {limit} OFFSET {offset}"
    );
    let mut stmt = conn.prepare(&list_query)?;
    let mut rows = stmt.query(params_from_iter(args.iter()))?;

    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row_to_summary(row)?);
    }
    Ok((out, total))
}

/// Returns the full logged record (including request/response bodies)
/// for one id, or `None` if no request with that id exists.
pub fn get_request(conn: &Connection, id: i64) -> Result<Option<Record>, StoreError> {
    let mut stmt = conn.prepare(
        "SELECT id, ts, route, virtual_key_label, real_key_label, format, model, path, stream,
            status_code, latency_ms, prompt_tokens, completion_tokens, total_tokens,
            cache_creation_tokens, cache_read_tokens, cost_usd, request_body, response_body, error,
            virtual_key_value, session_id, request_headers, response_headers
        FROM requests WHERE id = ?1",
    )?;
    let mut rows = stmt.query(rusqlite::params![id])?;
    let Some(row) = rows.next()? else {
        return Ok(None);
    };

    let ts_str: String = row.get(1)?;
    let stream: i64 = row.get(8)?;
    let record = Record {
        id: row.get(0)?,
        timestamp: parse_ts_string(&ts_str)?,
        route: row.get(2)?,
        virtual_key_label: row.get(3)?,
        real_key_label: row.get(4)?,
        format: row.get(5)?,
        model: row.get(6)?,
        path: row.get(7)?,
        stream: stream != 0,
        status_code: row.get(9)?,
        latency_ms: row.get(10)?,
        prompt_tokens: row.get(11)?,
        completion_tokens: row.get(12)?,
        total_tokens: row.get(13)?,
        cache_creation_tokens: row.get(14)?,
        cache_read_tokens: row.get(15)?,
        cost_usd: row.get(16)?,
        request_body: row.get::<_, Option<String>>(17)?.unwrap_or_default(),
        response_body: row.get::<_, Option<String>>(18)?.unwrap_or_default(),
        error: row.get::<_, Option<String>>(19)?.unwrap_or_default(),
        virtual_key_value: row.get(20)?,
        session_id: row.get(21)?,
        request_headers: row.get(22)?,
        response_headers: row.get(23)?,
    };
    Ok(Some(record))
}

/// Distinct model names seen in the request log, alphabetically sorted —
/// backs the model filter dropdown in the request browser. Unlike routes
/// and virtual keys, models aren't declared anywhere in config; they're
/// only known once traffic using them has been logged.
pub fn distinct_models(conn: &Connection) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare("SELECT DISTINCT model FROM requests ORDER BY model")?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row.get(0)?);
    }
    Ok(out)
}

/// Distinct virtual key labels seen in the request log — deliberately
/// queried from history rather than derived from `Config::routes[].keys`,
/// so labels of since-deleted or renamed keys remain filterable.
pub fn distinct_virtual_key_labels(conn: &Connection) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare("SELECT DISTINCT virtual_key_label FROM requests ORDER BY virtual_key_label")?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row.get(0)?);
    }
    Ok(out)
}

/// Distinct client-supplied `x-session-id` values seen in the request
/// log, alphabetically sorted, excluding the empty string (no header
/// sent) — backs the session filter dropdown in the request browser.
pub fn distinct_session_ids(conn: &Connection) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare("SELECT DISTINCT session_id FROM requests WHERE session_id != '' ORDER BY session_id")?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row.get(0)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{insert_record, open_connection};
    use chrono::TimeDelta;

    fn setup() -> Connection {
        let conn = open_connection(":memory:").unwrap();
        conn.execute_batch(super::super::SCHEMA).unwrap();
        conn
    }

    fn record(route: &str, model: &str, ts: DateTime<Utc>) -> Record {
        Record {
            timestamp: ts,
            route: route.to_string(),
            virtual_key_label: "vk1".to_string(),
            real_key_label: "rk1".to_string(),
            format: "openai".to_string(),
            model: model.to_string(),
            cost_usd: 0.5,
            ..Default::default()
        }
    }

    #[test]
    fn list_requests_filters_by_route_and_model() {
        let conn = setup();
        let now = Utc::now();
        insert_record(&conn, &record("r1", "gpt-4o", now)).unwrap();
        insert_record(&conn, &record("r2", "claude", now)).unwrap();

        let f = RequestFilter {
            since: Some(now - TimeDelta::hours(1)),
            route: Some("r1".to_string()),
            ..Default::default()
        };
        let (rows, total) = list_requests(&conn, &f, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].route, "r1");

        let f2 = RequestFilter {
            since: Some(now - TimeDelta::hours(1)),
            model: Some("claude".to_string()),
            ..Default::default()
        };
        let (rows2, _) = list_requests(&conn, &f2, 50, 0).unwrap();
        assert_eq!(rows2[0].model, "claude");
    }

    #[test]
    fn list_requests_orders_newest_first_and_paginates() {
        let conn = setup();
        let now = Utc::now();
        for i in 0..5 {
            insert_record(&conn, &record("r1", &format!("m{i}"), now)).unwrap();
        }
        let f = RequestFilter {
            since: Some(now - TimeDelta::hours(1)),
            ..Default::default()
        };
        let (page1, total) = list_requests(&conn, &f, 2, 0).unwrap();
        assert_eq!(total, 5);
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].model, "m4");
        assert_eq!(page1[1].model, "m3");

        let (page2, _) = list_requests(&conn, &f, 2, 2).unwrap();
        assert_eq!(page2[0].model, "m2");
    }

    #[test]
    fn list_requests_until_excludes_rows_after_the_upper_bound() {
        let conn = setup();
        let now = Utc::now();
        insert_record(&conn, &record("r1", "old", now - TimeDelta::hours(2))).unwrap();
        insert_record(&conn, &record("r1", "new", now)).unwrap();

        let f = RequestFilter {
            since: Some(now - TimeDelta::hours(3)),
            until: Some(now - TimeDelta::hours(1)),
            ..Default::default()
        };
        let (rows, total) = list_requests(&conn, &f, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].model, "old");
    }

    #[test]
    fn get_request_round_trips_virtual_key_value() {
        let conn = setup();
        let mut r = record("r1", "gpt-4o", Utc::now());
        r.virtual_key_value = "vk-abc123".to_string();
        insert_record(&conn, &r).unwrap();
        let id: i64 = conn.query_row("SELECT id FROM requests LIMIT 1", [], |row| row.get(0)).unwrap();

        let found = get_request(&conn, id).unwrap().unwrap();
        assert_eq!(found.virtual_key_value, "vk-abc123");
    }

    #[test]
    fn get_request_hit_and_miss() {
        let conn = setup();
        insert_record(&conn, &record("r1", "gpt-4o", Utc::now())).unwrap();
        let id: i64 = conn.query_row("SELECT id FROM requests LIMIT 1", [], |r| r.get(0)).unwrap();

        let found = get_request(&conn, id).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().model, "gpt-4o");

        let missing = get_request(&conn, id + 999).unwrap();
        assert!(missing.is_none());
    }

    #[test]
    fn distinct_models_and_virtual_key_labels_are_sorted_and_deduped() {
        let conn = setup();
        let now = Utc::now();
        insert_record(&conn, &record("r1", "gpt-4o", now)).unwrap();
        insert_record(&conn, &record("r1", "claude", now)).unwrap();
        insert_record(&conn, &record("r2", "claude", now)).unwrap();

        assert_eq!(distinct_models(&conn).unwrap(), vec!["claude".to_string(), "gpt-4o".to_string()]);
        assert_eq!(distinct_virtual_key_labels(&conn).unwrap(), vec!["vk1".to_string()]);
    }

    #[test]
    fn get_request_round_trips_session_id() {
        let conn = setup();
        let mut r = record("r1", "gpt-4o", Utc::now());
        r.session_id = "sess-abc".to_string();
        insert_record(&conn, &r).unwrap();
        let id: i64 = conn.query_row("SELECT id FROM requests LIMIT 1", [], |row| row.get(0)).unwrap();

        let found = get_request(&conn, id).unwrap().unwrap();
        assert_eq!(found.session_id, "sess-abc");
    }

    #[test]
    fn get_request_round_trips_request_and_response_headers() {
        let conn = setup();
        let mut r = record("r1", "gpt-4o", Utc::now());
        r.request_headers = "POST /v1/chat/completions HTTP/1.1\nauthorization: Bearer vk-abc\n".to_string();
        r.response_headers = "HTTP/1.1 200 OK\ncontent-type: application/json\n".to_string();
        insert_record(&conn, &r).unwrap();
        let id: i64 = conn.query_row("SELECT id FROM requests LIMIT 1", [], |row| row.get(0)).unwrap();

        let found = get_request(&conn, id).unwrap().unwrap();
        assert_eq!(found.request_headers, "POST /v1/chat/completions HTTP/1.1\nauthorization: Bearer vk-abc\n");
        assert_eq!(found.response_headers, "HTTP/1.1 200 OK\ncontent-type: application/json\n");
    }

    #[test]
    fn list_requests_filters_by_session_id() {
        let conn = setup();
        let now = Utc::now();
        let mut r1 = record("r1", "model-a", now);
        r1.session_id = "sess-a".to_string();
        let mut r2 = record("r1", "model-b", now);
        r2.session_id = "sess-b".to_string();
        insert_record(&conn, &r1).unwrap();
        insert_record(&conn, &r2).unwrap();

        let f = RequestFilter {
            since: Some(now - TimeDelta::hours(1)),
            session_id: Some("sess-a".to_string()),
            ..Default::default()
        };
        let (rows, total) = list_requests(&conn, &f, 50, 0).unwrap();
        assert_eq!(total, 1);
        assert_eq!(rows[0].model, "model-a");
    }

    #[test]
    fn distinct_session_ids_excludes_empty_and_dedupes() {
        let conn = setup();
        let now = Utc::now();
        let mut r1 = record("r1", "gpt-4o", now);
        r1.session_id = "sess-b".to_string();
        let mut r2 = record("r1", "gpt-4o", now);
        r2.session_id = "sess-a".to_string();
        let r3 = record("r1", "gpt-4o", now); // no session_id
        insert_record(&conn, &r1).unwrap();
        insert_record(&conn, &r2).unwrap();
        insert_record(&conn, &r3).unwrap();
        insert_record(&conn, &r2).unwrap(); // duplicate sess-a

        assert_eq!(distinct_session_ids(&conn).unwrap(), vec!["sess-a".to_string(), "sess-b".to_string()]);
    }
}
