use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use tantivy::{
	Index, IndexReader, IndexWriter, Term,
	schema::{INDEXED, STORED, STRING, Schema, SchemaBuilder, TEXT},
};

use crate::doc::SchemaFields;

/// Returns the directory path for the tantivy index.
///
/// Uses `$XDG_CACHE_HOME/ccq/tantivy` via `dirs::cache_dir()`,
/// falling back to `~/.cache/ccq/tantivy` if XDG is not set.
pub fn index_dir() -> PathBuf {
	dirs::cache_dir()
		.unwrap_or_else(|| {
			dirs::home_dir()
				.expect("could not determine home directory")
				.join(".cache")
		})
		.join("ccq")
		.join("tantivy")
}

/// Builds the tantivy schema for indexing Claude Code conversations.
pub fn build_schema() -> Schema {
	let mut builder: SchemaBuilder = Schema::builder();
	builder.add_text_field("id", STRING | STORED);
	builder.add_text_field("project", STRING | STORED);
	builder.add_text_field("project_name", STRING | STORED);
	builder.add_text_field("session_id", STRING | STORED);
	builder.add_text_field("git_branch", STRING | STORED);
	builder.add_text_field("role", STRING | STORED);
	builder.add_date_field("timestamp", INDEXED | STORED);
	builder.add_text_field("content", TEXT | STORED);
	builder.build()
}

/// Opens or creates the tantivy index at the default index directory.
///
/// If `force` is `true` and the directory already exists, it will be
/// removed and recreated from scratch.
pub fn open_or_create_index(force: bool) -> Result<Index> {
	let dir = index_dir();

	if force && dir.exists() {
		std::fs::remove_dir_all(&dir)
			.with_context(|| format!("failed to remove index directory: {}", dir.display()))?;
	}

	if !dir.exists() {
		std::fs::create_dir_all(&dir)
			.with_context(|| format!("failed to create index directory: {}", dir.display()))?;
	}

	let schema = build_schema();

	if force || !dir.join("meta.json").exists() {
		Index::create_in_dir(&dir, schema).context("failed to create tantivy index in directory")
	} else {
		Index::open_in_dir(&dir).context("failed to open tantivy index from directory")
	}
}

/// Cached handle for reading from the index.
///
/// Avoids repeated index opens, schema builds, and field resolution
/// when multiple queries are issued (e.g. in the TUI).
pub struct IndexHandle {
	pub fields: SchemaFields,
	pub index: Index,
	pub reader: IndexReader,
}

impl IndexHandle {
	pub fn open() -> Result<Self> {
		let dir = index_dir();
		if !dir.exists() || !dir.join("meta.json").exists() {
			bail!("No index found. Run `ccq index` first.");
		}
		let schema = build_schema();
		let fields = SchemaFields::resolve(&schema)?;
		let index = Index::open_in_dir(&dir).context("failed to open index")?;
		let reader = index.reader().context("failed to create index reader")?;
		Ok(Self { fields, index, reader })
	}

	pub fn searcher(&self) -> tantivy::Searcher {
		self.reader.searcher()
	}
}

/// Deletes all documents matching the given `session_id` from the index.
pub fn delete_session(writer: &IndexWriter, schema: &Schema, session_id: &str) -> Result<()> {
	let field = schema
		.get_field("session_id")
		.context("schema missing 'session_id' field")?;
	let term = Term::from_field_text(field, session_id);
	writer.delete_term(term);
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_schema_has_expected_fields() {
		let schema = build_schema();
		let expected_fields = [
			"id",
			"project",
			"project_name",
			"session_id",
			"git_branch",
			"role",
			"timestamp",
			"content",
		];
		for name in &expected_fields {
			assert!(schema.get_field(name).is_ok(), "schema is missing expected field: {name}",);
		}
	}

	#[test]
	fn test_index_dir_returns_path() {
		let dir = index_dir();
		let path_str = dir.to_string_lossy();
		assert!(
			path_str.ends_with("ccq/tantivy"),
			"expected path ending in ccq/tantivy, got: {path_str}",
		);
	}
}
