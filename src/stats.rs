use std::{collections::HashSet, fs, path::Path};

use anyhow::{Context, Result, bail};
use tantivy::{
	Index,
	schema::TantivyDocument,
};

use crate::{
	doc::{self, SchemaFields},
	index::{build_schema, index_dir},
	meta::IndexMeta,
	tui::theme,
};

/// Display statistics about the current index.
pub fn run_stats(json: bool) -> Result<()> {
	let idx_dir = index_dir();

	if !idx_dir.exists() || !idx_dir.join("meta.json").exists() {
		bail!("no index found at {}. Run `ccq index` first.", idx_dir.display(),);
	}

	let index = Index::open_in_dir(&idx_dir).context("failed to open tantivy index")?;
	let schema = build_schema();
	let reader = index.reader().context("failed to create index reader")?;
	let searcher = reader.searcher();

	let fields = SchemaFields::resolve(&schema)?;

	let mut projects: HashSet<String> = HashSet::new();
	let mut sessions: HashSet<String> = HashSet::new();
	let mut total_messages: usize = 0;

	// Iterate over all segments and all documents within each segment.
	for segment_reader in searcher.segment_readers() {
		let store_reader = segment_reader
			.get_store_reader(64)
			.context("failed to open segment store reader")?;

		for doc_id in 0..segment_reader.max_doc() {
			// Skip deleted documents.
			if segment_reader.is_deleted(doc_id) {
				continue;
			}

			let d: TantivyDocument = store_reader
				.get(doc_id)
				.context("failed to retrieve document from store")?;

			let project_name = doc::get_text(&d, fields.project_name);
			let session_id = doc::get_text(&d, fields.session_id);

			if !project_name.is_empty() {
				projects.insert(project_name);
			}
			if !session_id.is_empty() {
				sessions.insert(session_id);
			}

			total_messages += 1;
		}
	}

	// Calculate index size on disk.
	let size = dir_size(&idx_dir).context("failed to calculate index size")?;

	// Load IndexMeta to find the latest mtime for "last updated".
	let meta = IndexMeta::load(&idx_dir).context("failed to load index metadata")?;
	let last_updated = meta.files.values().copied().max().unwrap_or(0);

	let last_updated_str = if last_updated > 0 {
		chrono::DateTime::from_timestamp(last_updated as i64, 0)
			.map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
			.unwrap_or_else(|| "unknown".to_string())
	} else {
		"unknown".to_string()
	};

	if json {
		let stats = serde_json::json!({
			"projects_indexed": projects.len(),
			"sessions_indexed": sessions.len(),
			"messages_indexed": total_messages,
			"index_size_bytes": size,
			"index_size_human": format_bytes(size),
			"last_updated": last_updated_str,
		});
		println!("{}", serde_json::to_string_pretty(&stats)?);
		return Ok(());
	}

	println!(
		"{} {}",
		theme::styled_bold("Projects indexed:"),
		theme::styled_project(&projects.len().to_string()),
	);
	println!(
		"{} {}",
		theme::styled_bold("Sessions indexed:"),
		theme::styled_project(&sessions.len().to_string()),
	);
	println!(
		"{} {}",
		theme::styled_bold("Messages indexed:"),
		theme::styled_project(&total_messages.to_string()),
	);
	println!(
		"{} {}",
		theme::styled_bold("Index size:      "),
		theme::styled_project(&format_bytes(size)),
	);
	println!(
		"{} {}",
		theme::styled_bold("Last updated:    "),
		theme::styled_project(&last_updated_str),
	);

	Ok(())
}

/// Recursively compute the total size (in bytes) of all files under
/// the given directory.
fn dir_size(path: &Path) -> Result<u64> {
	let mut total: u64 = 0;
	if path.is_file() {
		return Ok(fs::metadata(path).map(|m| m.len()).unwrap_or(0));
	}
	for entry in
		fs::read_dir(path).with_context(|| format!("failed to read directory {}", path.display()))?
	{
		let entry = entry?;
		let ft = entry.file_type()?;
		if ft.is_file() {
			total += entry.metadata()?.len();
		} else if ft.is_dir() {
			total += dir_size(&entry.path())?;
		}
	}
	Ok(total)
}

/// Format a byte count into a human-readable string (B, KB, MB, GB).
fn format_bytes(bytes: u64) -> String {
	const KB: u64 = 1024;
	const MB: u64 = 1024 * KB;
	const GB: u64 = 1024 * MB;

	if bytes >= GB {
		format!("{:.1} GB", bytes as f64 / GB as f64)
	} else if bytes >= MB {
		format!("{:.1} MB", bytes as f64 / MB as f64)
	} else if bytes >= KB {
		format!("{:.1} KB", bytes as f64 / KB as f64)
	} else {
		format!("{} B", bytes)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_format_bytes() {
		assert_eq!(format_bytes(0), "0 B");
		assert_eq!(format_bytes(500), "500 B");
		assert_eq!(format_bytes(1024), "1.0 KB");
		assert_eq!(format_bytes(1536), "1.5 KB");
		assert_eq!(format_bytes(1_048_576), "1.0 MB");
		assert_eq!(format_bytes(1_073_741_824), "1.0 GB");
	}

	#[test]
	fn test_dir_size_single_file() {
		let tmp = std::env::temp_dir().join("ccq-test-dir-size");
		let _ = fs::remove_dir_all(&tmp);
		fs::create_dir_all(&tmp).unwrap();

		let file_path = tmp.join("test.txt");
		fs::write(&file_path, "hello world").unwrap();

		let size = dir_size(&tmp).unwrap();
		assert_eq!(size, 11); // "hello world" is 11 bytes

		let _ = fs::remove_dir_all(&tmp);
	}
}
