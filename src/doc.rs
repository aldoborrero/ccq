use anyhow::{Context, Result};
use tantivy::schema::{Field, OwnedValue, Schema};

/// Pre-resolved schema field handles.
///
/// Resolving fields once avoids repeated lookups and replaces `.unwrap()`
/// calls with a single fallible constructor that produces clear error
/// messages when the index is corrupted or the schema has changed.
pub struct SchemaFields {
	pub id: Field,
	pub project: Field,
	pub project_name: Field,
	pub session_id: Field,
	pub git_branch: Field,
	pub role: Field,
	pub timestamp: Field,
	pub content: Field,
}

impl SchemaFields {
	pub fn resolve(schema: &Schema) -> Result<Self> {
		Ok(Self {
			id: schema
				.get_field("id")
				.context("schema missing 'id' field (corrupted index?)")?,
			project: schema
				.get_field("project")
				.context("schema missing 'project' field (corrupted index?)")?,
			project_name: schema
				.get_field("project_name")
				.context("schema missing 'project_name' field (corrupted index?)")?,
			session_id: schema
				.get_field("session_id")
				.context("schema missing 'session_id' field (corrupted index?)")?,
			git_branch: schema
				.get_field("git_branch")
				.context("schema missing 'git_branch' field (corrupted index?)")?,
			role: schema
				.get_field("role")
				.context("schema missing 'role' field (corrupted index?)")?,
			timestamp: schema
				.get_field("timestamp")
				.context("schema missing 'timestamp' field (corrupted index?)")?,
			content: schema
				.get_field("content")
				.context("schema missing 'content' field (corrupted index?)")?,
		})
	}
}

/// Extract a text field from a document, returning an empty string if missing.
pub fn get_text(doc: &tantivy::TantivyDocument, field: Field) -> String {
	match doc.get_first(field) {
		Some(OwnedValue::Str(s)) => s.clone(),
		_ => String::new(),
	}
}

/// Extract a tantivy DateTime from a document, returning None if missing.
pub fn get_datetime(doc: &tantivy::TantivyDocument, field: Field) -> Option<tantivy::DateTime> {
	match doc.get_first(field) {
		Some(OwnedValue::Date(dt)) => Some(*dt),
		_ => None,
	}
}

/// Format a tantivy DateTime as `YYYY-MM-DD HH:MM:SS`.
pub fn format_datetime(dt: tantivy::DateTime) -> String {
	let secs = dt.into_timestamp_secs();
	match chrono::DateTime::from_timestamp(secs, 0) {
		Some(t) => t.format("%Y-%m-%d %H:%M:%S").to_string(),
		None => "unknown".to_string(),
	}
}

/// Format a tantivy DateTime as `YYYY-MM-DD HH:MM` (no seconds).
pub fn format_datetime_short(dt: tantivy::DateTime) -> String {
	let secs = dt.into_timestamp_secs();
	match chrono::DateTime::from_timestamp(secs, 0) {
		Some(t) => t.format("%Y-%m-%d %H:%M").to_string(),
		None => "unknown".to_string(),
	}
}

/// Format a tantivy DateTime as a date-only string `YYYY-MM-DD`.
pub fn format_date(dt: tantivy::DateTime) -> String {
	let secs = dt.into_timestamp_secs();
	match chrono::DateTime::from_timestamp(secs, 0) {
		Some(t) => t.format("%Y-%m-%d").to_string(),
		None => "unknown".to_string(),
	}
}
