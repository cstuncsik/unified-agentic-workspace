use uuid::Uuid;

/// Generate a new random identifier (UUID v4) for a domain record.
pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

/// Current UTC timestamp as an RFC 3339 string, used for `created_at`/`updated_at` columns.
pub fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}
