use std::path::Path;

use rusqlite::Connection;

use crate::util::now_rfc3339;

/// Ordered list of schema migrations: `(version, name, SQL body)`.
/// Migrations are applied in order; each must have a strictly increasing version.
const MIGRATIONS: &[(i64, &str, &str)] = &[(
    1,
    "init_workspaces",
    include_str!("migrations/0001_init.sql"),
)];

/// Open (creating if needed) the SQLite database at `db_path`, enable foreign keys,
/// and apply any pending migrations.
pub fn init_db(db_path: &Path) -> rusqlite::Result<Connection> {
    let mut conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    run_migrations(&mut conn)?;
    Ok(conn)
}

/// Apply every migration whose version is greater than the highest recorded version.
/// Each migration runs inside its own transaction so it is applied atomically.
pub(crate) fn run_migrations(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY NOT NULL,
            name       TEXT NOT NULL,
            applied_at TEXT NOT NULL
        );",
    )?;

    let applied: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;

    for (version, name, sql) in MIGRATIONS {
        if *version > applied {
            let tx = conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![version, name, now_rfc3339()],
            )?;
            tx.commit()?;
        }
    }

    Ok(())
}
