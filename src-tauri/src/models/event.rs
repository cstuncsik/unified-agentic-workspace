use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};

use crate::util::now_rfc3339;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub workspace_id: String,
    pub r#type: String,
    pub payload_json: String,
    pub created_at: String,
}

const COLUMNS: &str = "id, workspace_id, type, payload_json, created_at";

fn from_row(row: &Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get("id")?,
        workspace_id: row.get("workspace_id")?,
        r#type: row.get("type")?,
        payload_json: row.get("payload_json")?,
        created_at: row.get("created_at")?,
    })
}

pub fn create(
    conn: &Connection,
    id: &str,
    workspace_id: &str,
    event_type: &str,
    payload_json: &str,
) -> rusqlite::Result<Event> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT INTO events (id, workspace_id, type, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, workspace_id, event_type, payload_json, now],
    )?;
    Ok(get(conn, id)?.expect("event exists immediately after insert"))
}

pub fn get(conn: &Connection, id: &str) -> rusqlite::Result<Option<Event>> {
    let sql = format!("SELECT {COLUMNS} FROM events WHERE id = ?1");
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], from_row)?;
    match rows.next() {
        Some(row) => Ok(Some(row?)),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::workspace;
    use crate::util::new_id;

    fn migrated_conn() -> Connection {
        let mut conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;")
            .expect("enable foreign keys");
        crate::db::run_migrations(&mut conn).expect("run migrations");
        conn
    }

    #[test]
    fn create_then_get() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let e = create(
            &conn,
            &new_id(),
            &ws,
            "coding_workspace.completed",
            r#"{"checks_passed":false}"#,
        )
        .unwrap();
        assert_eq!(e.workspace_id, ws);
        assert_eq!(e.r#type, "coding_workspace.completed");
        assert!(e.payload_json.contains("checks_passed"));
        assert!(get(&conn, &e.id).unwrap().is_some());
    }

    #[test]
    fn deleting_workspace_cascades_events() {
        let conn = migrated_conn();
        let ws = workspace::create(&conn, "Test", "mixed").unwrap().id;
        let e = create(&conn, &new_id(), &ws, "x", "{}").unwrap();
        workspace::delete(&conn, &ws).unwrap();
        assert!(get(&conn, &e.id).unwrap().is_none());
    }
}
