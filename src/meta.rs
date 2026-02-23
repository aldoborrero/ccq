use std::{
	collections::HashMap,
	fs,
	path::{Path, PathBuf},
	time::SystemTime,
};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Tracks file modification times so incremental indexing can skip
/// sessions whose JSONL files have not changed since the last run.
#[derive(Serialize, Deserialize, Default)]
pub struct IndexMeta {
	/// Maps session file path (as string) to its last-known mtime
	/// (seconds since UNIX epoch).
	pub files: HashMap<String, u64>,
}

impl IndexMeta {
	/// Load meta.json from the parent of the tantivy index directory.
	///
	/// Returns a default (empty) `IndexMeta` if the file does not exist.
	pub fn load(index_dir: &Path) -> Result<Self> {
		let path = Self::path(index_dir);
		if !path.exists() {
			return Ok(Self::default());
		}
		let contents =
			fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
		serde_json::from_str(&contents).with_context(|| format!("failed to parse {}", path.display()))
	}

	/// Persist meta.json to the parent of the tantivy index directory.
	pub fn save(&self, index_dir: &Path) -> Result<()> {
		let path = Self::path(index_dir);
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent)
				.with_context(|| format!("failed to create directory {}", parent.display()))?;
		}
		let json = serde_json::to_string_pretty(self).context("failed to serialize index meta")?;
		fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))
	}

	/// Returns the path to meta.json — stored in the parent of the
	/// tantivy index directory (e.g. `$XDG_CACHE_HOME/ccq/meta.json`).
	fn path(index_dir: &Path) -> PathBuf {
		index_dir
			.parent()
			.expect("index_dir must have a parent")
			.join("meta.json")
	}

	/// Convert a `SystemTime` mtime to a u64 (seconds since UNIX epoch).
	pub fn mtime_to_u64(mtime: SystemTime) -> u64 {
		mtime
			.duration_since(SystemTime::UNIX_EPOCH)
			.unwrap_or_default()
			.as_secs()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_load_missing_file_returns_default() {
		let tmp = std::env::temp_dir().join("ccq-test-meta-load");
		let _ = fs::remove_dir_all(&tmp);
		fs::create_dir_all(&tmp).unwrap();

		let meta = IndexMeta::load(&tmp.join("tantivy")).unwrap();
		assert!(meta.files.is_empty());

		let _ = fs::remove_dir_all(&tmp);
	}

	#[test]
	fn test_save_and_load_roundtrip() {
		let tmp = std::env::temp_dir().join("ccq-test-meta-roundtrip");
		let _ = fs::remove_dir_all(&tmp);
		fs::create_dir_all(&tmp).unwrap();

		let index_dir = tmp.join("tantivy");

		let mut meta = IndexMeta::default();
		meta
			.files
			.insert("/some/path/session.jsonl".to_string(), 1700000000);
		meta.save(&index_dir).unwrap();

		let loaded = IndexMeta::load(&index_dir).unwrap();
		assert_eq!(loaded.files.len(), 1);
		assert_eq!(loaded.files.get("/some/path/session.jsonl"), Some(&1700000000),);

		let _ = fs::remove_dir_all(&tmp);
	}

	#[test]
	fn test_meta_path_is_in_parent() {
		let path = IndexMeta::path(Path::new("/cache/ccq/tantivy"));
		assert_eq!(path, PathBuf::from("/cache/ccq/meta.json"));
	}

	#[test]
	fn test_mtime_to_u64() {
		let epoch = SystemTime::UNIX_EPOCH;
		assert_eq!(IndexMeta::mtime_to_u64(epoch), 0);

		let later = epoch + std::time::Duration::from_secs(1_700_000_000);
		assert_eq!(IndexMeta::mtime_to_u64(later), 1_700_000_000);
	}
}
