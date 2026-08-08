use std::collections::HashMap;

use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use thiserror::Error;

use super::{parse_ts_string, to_ts_string};

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("groupBy must be \"model\", \"route\", \"virtual_key\", or \"real_key\", got {0:?}")]
    InvalidGroupBy(String),
    #[error("metric must be \"count\", \"total_tokens\", or \"cost_usd\", got {0:?}")]
    InvalidMetric(String),
    #[error("bucket must be positive")]
    InvalidBucket,
    #[error("parse timestamp {0:?}: {1}")]
    Timestamp(String, chrono::ParseError),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
}

/// One aggregated row grouped by model/route/virtual_key/real_key.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StatRow {
    pub group: String,
    pub count: i64,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cache_read_tokens: i64,
    /// `cache_read_tokens / prompt_tokens` (0 when `prompt_tokens` is 0).
    /// `prompt_tokens` is always the total input token count including
    /// cache tokens, so this is comparable across OpenAI and Anthropic
    /// routes within the same group.
    pub cache_hit_rate: f64,
    pub avg_latency_ms: f64,
    pub errors: i64,
    pub total_cost_usd: f64,
}

/// Maps a group-by value to its backing SQL column, shared by
/// [`stats`] and [`time_series`] so the two stay in sync.
pub fn group_by_column(group_by: &str) -> Result<&'static str, StoreError> {
    match group_by {
        "model" => Ok("model"),
        "route" => Ok("route"),
        "virtual_key" => Ok("virtual_key_label"),
        "real_key" => Ok("real_key_label"),
        other => Err(StoreError::InvalidGroupBy(other.to_string())),
    }
}

/// Aggregates logged requests in `[since, until]` (an unbounded `until`
/// means "up to now"), grouped by "model", "route", "virtual_key", or
/// "real_key".
pub fn stats(conn: &Connection, group_by: &str, since: DateTime<Utc>, until: Option<DateTime<Utc>>) -> Result<Vec<StatRow>, StoreError> {
    let col = group_by_column(group_by)?;
    let until_clause = if until.is_some() { "AND ts <= ?2" } else { "" };
    let query = format!(
        "SELECT
            {col} AS grp,
            COUNT(*) AS count,
            SUM(prompt_tokens),
            SUM(completion_tokens),
            SUM(total_tokens),
            SUM(cache_creation_tokens),
            SUM(cache_read_tokens),
            AVG(latency_ms),
            SUM(CASE WHEN status_code >= 400 OR status_code = 0 THEN 1 ELSE 0 END),
            SUM(cost_usd)
        FROM requests
        WHERE ts >= ?1 {until_clause}
        GROUP BY {col}
        ORDER BY count DESC"
    );

    let mut stmt = conn.prepare(&query)?;
    let mut rows = match until {
        Some(u) => stmt.query(params![to_ts_string(since), to_ts_string(u)])?,
        None => stmt.query(params![to_ts_string(since)])?,
    };

    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let mut r = StatRow {
            group: row.get(0)?,
            count: row.get(1)?,
            prompt_tokens: row.get(2)?,
            completion_tokens: row.get(3)?,
            total_tokens: row.get(4)?,
            cache_creation_tokens: row.get(5)?,
            cache_read_tokens: row.get(6)?,
            avg_latency_ms: row.get(7)?,
            errors: row.get(8)?,
            total_cost_usd: row.get(9)?,
            cache_hit_rate: 0.0,
        };
        if r.prompt_tokens > 0 {
            r.cache_hit_rate = r.cache_read_tokens as f64 / r.prompt_tokens as f64;
        }
        out.push(r);
    }
    Ok(out)
}

/// A regular grid of time buckets covering `[since, now]`, with one
/// aligned value slice per group label — ready to feed straight into a
/// chart (missing buckets are filled with 0, not omitted).
#[derive(Debug, Clone, Default, Serialize)]
pub struct TimeSeriesResult {
    pub buckets: Vec<DateTime<Utc>>,
    pub series: HashMap<String, Vec<f64>>,
}

/// Picks a bucket width proportional to the look-back window, so a
/// chart never ends up with too few or too many points to be useful.
pub fn default_bucket(since: TimeDelta) -> TimeDelta {
    if since <= TimeDelta::hours(6) {
        TimeDelta::minutes(5)
    } else if since <= TimeDelta::hours(48) {
        TimeDelta::hours(1)
    } else {
        TimeDelta::hours(24)
    }
}

fn truncate_to_bucket(ts: DateTime<Utc>, bucket: TimeDelta) -> DateTime<Utc> {
    let bucket_ns = bucket.num_nanoseconds().unwrap_or(1).max(1);
    let ts_ns = ts.timestamp_nanos_opt().unwrap_or(0);
    DateTime::from_timestamp_nanos(ts_ns.div_euclid(bucket_ns) * bucket_ns)
}

/// Buckets logged requests in `[since, until]` (an unbounded `until`
/// means "up to now") into fixed-width windows, grouped by
/// "model"/"route"/"virtual_key"/"real_key". `metric` selects what's
/// summed per bucket: "count", "total_tokens", or "cost_usd". Bucketing
/// happens in application code (rather than in SQL) to keep the
/// bucket-edge behavior identical to [`stats`]'s own timestamp parsing,
/// rather than relying on SQLite's date functions.
pub fn time_series(
    conn: &Connection,
    group_by: &str,
    metric: &str,
    since: DateTime<Utc>,
    until: Option<DateTime<Utc>>,
    bucket: TimeDelta,
) -> Result<TimeSeriesResult, StoreError> {
    let col = group_by_column(group_by)?;
    if !matches!(metric, "count" | "total_tokens" | "cost_usd") {
        return Err(StoreError::InvalidMetric(metric.to_string()));
    }
    if bucket <= TimeDelta::zero() {
        return Err(StoreError::InvalidBucket);
    }

    let until_clause = if until.is_some() { "AND ts <= ?2" } else { "" };
    let query = format!("SELECT ts, {col} AS grp, total_tokens, cost_usd FROM requests WHERE ts >= ?1 {until_clause} ORDER BY ts");
    let mut stmt = conn.prepare(&query)?;
    let mut rows = match until {
        Some(u) => stmt.query(params![to_ts_string(since), to_ts_string(u)])?,
        None => stmt.query(params![to_ts_string(since)])?,
    };

    let mut sums: HashMap<DateTime<Utc>, HashMap<String, f64>> = HashMap::new();
    while let Some(row) = rows.next()? {
        let ts_str: String = row.get(0)?;
        let grp: String = row.get(1)?;
        let total_tokens: i64 = row.get(2)?;
        let cost_usd: f64 = row.get(3)?;

        let ts = parse_ts_string(&ts_str)?;
        let b = truncate_to_bucket(ts, bucket);
        let value = match metric {
            "total_tokens" => total_tokens as f64,
            "cost_usd" => cost_usd,
            _ => 1.0,
        };
        *sums.entry(b).or_default().entry(grp).or_insert(0.0) += value;
    }

    let mut groups: Vec<String> = sums.values().flat_map(|by_group| by_group.keys().cloned()).collect();
    groups.sort();
    groups.dedup();

    let mut result = TimeSeriesResult {
        buckets: Vec::new(),
        series: groups.iter().map(|g| (g.clone(), Vec::new())).collect(),
    };

    let end = until.unwrap_or_else(Utc::now);
    let mut b = truncate_to_bucket(since, bucket);
    while b <= end {
        result.buckets.push(b);
        for g in &groups {
            let v = sums.get(&b).and_then(|m| m.get(g)).copied().unwrap_or(0.0);
            result.series.get_mut(g).unwrap().push(v);
        }
        b += bucket;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{insert_record, open_connection};

    fn setup() -> Connection {
        let conn = open_connection(":memory:").unwrap();
        conn.execute_batch(super::super::SCHEMA).unwrap();
        conn
    }

    fn record(model: &str, ts: DateTime<Utc>, status: i64, cost: f64) -> super::super::Record {
        super::super::Record {
            timestamp: ts,
            route: "r1".to_string(),
            virtual_key_label: "vk1".to_string(),
            real_key_label: "rk1".to_string(),
            format: "openai".to_string(),
            model: model.to_string(),
            status_code: status,
            latency_ms: 100,
            prompt_tokens: 10,
            completion_tokens: 5,
            total_tokens: 15,
            cost_usd: cost,
            ..Default::default()
        }
    }

    #[test]
    fn stats_aggregates_by_model_and_sums_cost() {
        let conn = setup();
        let now = Utc::now();
        insert_record(&conn, &record("gpt-4o", now, 200, 0.01)).unwrap();
        insert_record(&conn, &record("gpt-4o", now, 200, 0.02)).unwrap();
        insert_record(&conn, &record("claude", now, 500, 0.03)).unwrap();

        let rows = stats(&conn, "model", now - TimeDelta::hours(1), None).unwrap();
        let gpt = rows.iter().find(|r| r.group == "gpt-4o").unwrap();
        assert_eq!(gpt.count, 2);
        assert!((gpt.total_cost_usd - 0.03).abs() < 1e-9);

        let claude = rows.iter().find(|r| r.group == "claude").unwrap();
        assert_eq!(claude.errors, 1);
    }

    #[test]
    fn stats_until_excludes_rows_after_the_upper_bound() {
        let conn = setup();
        let now = Utc::now();
        insert_record(&conn, &record("gpt-4o", now - TimeDelta::hours(2), 200, 0.01)).unwrap();
        insert_record(&conn, &record("gpt-4o", now, 200, 0.02)).unwrap();

        let rows = stats(&conn, "model", now - TimeDelta::hours(3), Some(now - TimeDelta::hours(1))).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].count, 1);
        assert!((rows[0].total_cost_usd - 0.01).abs() < 1e-9);
    }

    #[test]
    fn stats_rejects_unknown_group_by() {
        let conn = setup();
        assert!(stats(&conn, "bogus", Utc::now(), None).is_err());
    }

    #[test]
    fn stats_cache_hit_rate_is_zero_with_no_prompt_tokens() {
        let conn = setup();
        let now = Utc::now();
        let mut r = record("m", now, 200, 0.0);
        r.prompt_tokens = 0;
        r.total_tokens = 0;
        insert_record(&conn, &r).unwrap();
        let rows = stats(&conn, "model", now - TimeDelta::hours(1), None).unwrap();
        assert_eq!(rows[0].cache_hit_rate, 0.0);
    }

    #[test]
    fn time_series_buckets_cost_metric() {
        let conn = setup();
        let now = Utc::now();
        insert_record(&conn, &record("gpt-4o", now, 200, 0.5)).unwrap();

        let result = time_series(&conn, "model", "cost_usd", now - TimeDelta::hours(1), None, TimeDelta::minutes(5)).unwrap();
        let total: f64 = result.series["gpt-4o"].iter().sum();
        assert!((total - 0.5).abs() < 1e-9);
    }

    #[test]
    fn time_series_until_stops_bucketing_at_the_upper_bound() {
        let conn = setup();
        let now = Utc::now();
        insert_record(&conn, &record("gpt-4o", now - TimeDelta::hours(2), 200, 0.5)).unwrap();
        insert_record(&conn, &record("gpt-4o", now, 200, 1.5)).unwrap();

        let result = time_series(
            &conn,
            "model",
            "cost_usd",
            now - TimeDelta::hours(3),
            Some(now - TimeDelta::hours(1)),
            TimeDelta::minutes(5),
        )
        .unwrap();
        let total: f64 = result.series["gpt-4o"].iter().sum();
        assert!((total - 0.5).abs() < 1e-9);
        assert!(*result.buckets.last().unwrap() <= now - TimeDelta::hours(1));
    }

    #[test]
    fn time_series_rejects_invalid_metric() {
        let conn = setup();
        assert!(time_series(&conn, "model", "bogus", Utc::now(), None, TimeDelta::minutes(5)).is_err());
    }

    #[test]
    fn time_series_rejects_non_positive_bucket() {
        let conn = setup();
        assert!(time_series(&conn, "model", "count", Utc::now(), None, TimeDelta::zero()).is_err());
    }

    #[test]
    fn default_bucket_scales_with_window() {
        assert_eq!(default_bucket(TimeDelta::hours(1)), TimeDelta::minutes(5));
        assert_eq!(default_bucket(TimeDelta::hours(24)), TimeDelta::hours(1));
        assert_eq!(default_bucket(TimeDelta::days(7)), TimeDelta::hours(24));
    }
}
