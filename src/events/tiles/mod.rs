//! Tiles projection — derives a minimal set of "agent instance" tiles
//! from the last N hours of events on demand. See spec
//! 2026-04-23-tiles-projection-design.md.

pub mod cluster;
pub mod derive;
pub mod surfaces;
pub mod tile;

use crate::events::store::query::EventRow;
use rusqlite::Connection;

/// Compute the tile projection over events with received_at >= since_ms.
/// Pure function over the events table — no caching, no incremental
/// state. Each call re-scans and re-derives.
///
/// Returns tiles sorted by last_seen_at descending.
pub fn compute(conn: &Connection, since_ms: i64) -> rusqlite::Result<Vec<tile::Tile>> {
    let rows = fetch_since(conn, since_ms)?;
    let derived = walk_and_derive(&rows);
    let tiles = cluster::group(&derived);
    Ok(tiles)
}

/// Fetch all events with received_at >= since_ms, ordered ASC by
/// received_at, then by id ASC for stable tiebreaking.
fn fetch_since(conn: &Connection, since_ms: i64) -> rusqlite::Result<Vec<EventRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, received_at, event_id, event_type, source, session_id, project,
                host, os_user, device_id, event_ts, seq, source_pid, schema_version,
                correlation, context, payload
         FROM events
         WHERE received_at >= ?
         ORDER BY received_at ASC, id ASC",
    )?;
    let rows_iter = stmt.query_map([since_ms], |row| {
        Ok(EventRow {
            id: row.get(0)?,
            received_at: row.get(1)?,
            event_id: row.get(2)?,
            event_type: row.get(3)?,
            source: row.get(4)?,
            session_id: row.get(5)?,
            project: row.get(6)?,
            host: row.get(7)?,
            os_user: row.get(8)?,
            device_id: row.get(9)?,
            event_ts: row.get(10)?,
            seq: row.get(11)?,
            source_pid: row.get(12)?,
            schema_version: row.get(13)?,
            correlation: row.get::<_, Option<String>>(14)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            context: row.get::<_, Option<String>>(15)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            payload: row.get::<_, Option<String>>(16)?
                .and_then(|s| serde_json::from_str(&s).ok()),
        })
    })?;
    let mut out = Vec::new();
    for r in rows_iter {
        out.push(r?);
    }
    Ok(out)
}

/// Single-pass walk: maintain a rolling VS Code "currently visible
/// view per window" map; for each row, update the map if it's a
/// view.visible event, then call derive() with the current map state.
/// Returns all DerivedRows that successfully derived.
fn walk_and_derive(rows: &[EventRow]) -> Vec<derive::DerivedRow> {
    let mut active_views = derive::VscodeAttribution::new();
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some((window_pid, view, visible)) = derive::parse_view_visible_change(row) {
            if visible {
                active_views.insert(window_pid, view);
            } else {
                active_views.remove(&window_pid);
            }
        }
        if let Some(d) = derive::derive(row, &active_views) {
            out.push(d);
        }
    }
    out
}
