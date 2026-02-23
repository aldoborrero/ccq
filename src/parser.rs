use anyhow::Result;
use serde::Deserialize;

/// A parsed message ready for indexing
pub struct ParsedMessage {
	pub session_id: String,
	pub message_uuid: String,
	pub project: String,
	pub project_name: String,
	pub timestamp: String,
	pub git_branch: String,
	pub role: String,
	pub content: String,
}

/// Raw content block from the JSONL message arrays
#[derive(Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
	#[serde(rename = "text")]
	Text { text: String },
	#[serde(rename = "thinking")]
	Thinking {},
	#[serde(rename = "tool_use")]
	ToolUse {},
	#[serde(rename = "tool_result")]
	ToolResult {},
	#[serde(other)]
	Other,
}

/// Content can be a plain string or an array of blocks
#[derive(Deserialize)]
#[serde(untagged)]
enum Content {
	String(String),
	Array(Vec<ContentBlock>),
}

/// The inner message object
#[derive(Deserialize)]
struct Message {
	role: String,
	content: Content,
}

/// Top-level JSONL line
#[derive(Deserialize)]
struct RawLine {
	#[serde(rename = "type")]
	msg_type: String,
	#[serde(rename = "sessionId", default)]
	session_id: String,
	#[serde(default)]
	uuid: String,
	#[serde(default)]
	timestamp: String,
	#[serde(rename = "gitBranch", default)]
	git_branch: String,
	#[serde(default)]
	message: Option<Message>,
}

/// Parse a single JSONL line into a `ParsedMessage` if it contains
/// indexable content.
///
/// Returns `Ok(None)` for message types we intentionally skip
/// (progress, file-history-snapshot, system) or when no extractable
/// text content is found.
pub fn parse_line(line: &str, project: &str, project_name: &str) -> Result<Option<ParsedMessage>> {
	let raw: RawLine = serde_json::from_str(line)?;

	// Only index user and assistant messages
	match raw.msg_type.as_str() {
		"user" | "assistant" => {},
		_ => return Ok(None),
	}

	let message = match raw.message {
		Some(m) => m,
		None => return Ok(None),
	};

	let text = extract_text(&message.content);

	if text.is_empty() {
		return Ok(None);
	}

	Ok(Some(ParsedMessage {
		session_id: raw.session_id,
		message_uuid: raw.uuid,
		project: project.to_string(),
		project_name: project_name.to_string(),
		timestamp: raw.timestamp,
		git_branch: raw.git_branch,
		role: message.role,
		content: text,
	}))
}

/// Extract indexable text from message content.
///
/// For string content, returns it directly (unless it is the
/// placeholder). For array content, concatenates all "text" blocks
/// while skipping thinking, tool_use, and tool_result blocks.
fn extract_text(content: &Content) -> String {
	match content {
		Content::String(s) => {
			if s == "(no content)" {
				String::new()
			} else {
				s.clone()
			}
		},
		Content::Array(blocks) => {
			let parts: Vec<&str> = blocks
				.iter()
				.filter_map(|block| match block {
					ContentBlock::Text { text } if text != "(no content)" => Some(text.as_str()),
					_ => None,
				})
				.collect();
			parts.join("\n\n")
		},
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn make_line(json: &str) -> String {
		json.to_string()
	}

	#[test]
	fn test_parse_user_message_string_content() {
		let line = make_line(
			r#"{
				"type": "user",
				"sessionId": "sess-001",
				"uuid": "msg-001",
				"timestamp": "2026-01-14T10:00:00Z",
				"gitBranch": "main",
				"message": {
					"role": "user",
					"content": "Hello, how are you?"
				}
			}"#,
		);
		let result = parse_line(&line, "/home/user/project", "my-project")
			.unwrap()
			.unwrap();
		assert_eq!(result.session_id, "sess-001");
		assert_eq!(result.message_uuid, "msg-001");
		assert_eq!(result.project, "/home/user/project");
		assert_eq!(result.project_name, "my-project");
		assert_eq!(result.timestamp, "2026-01-14T10:00:00Z");
		assert_eq!(result.git_branch, "main");
		assert_eq!(result.role, "user");
		assert_eq!(result.content, "Hello, how are you?");
	}

	#[test]
	fn test_parse_assistant_text_block() {
		let line = make_line(
			r#"{
				"type": "assistant",
				"sessionId": "sess-002",
				"uuid": "msg-002",
				"timestamp": "2026-01-14T10:01:00Z",
				"gitBranch": "develop",
				"message": {
					"role": "assistant",
					"content": [
						{"type": "text", "text": "Here is my response."}
					]
				}
			}"#,
		);
		let result = parse_line(&line, "/home/user/project", "my-project")
			.unwrap()
			.unwrap();
		assert_eq!(result.role, "assistant");
		assert_eq!(result.content, "Here is my response.");
	}

	#[test]
	fn test_skip_thinking_blocks() {
		let line = make_line(
			r#"{
				"type": "assistant",
				"sessionId": "sess-003",
				"uuid": "msg-003",
				"timestamp": "2026-01-14T10:02:00Z",
				"gitBranch": "main",
				"message": {
					"role": "assistant",
					"content": [
						{"type": "thinking", "thinking": "Let me think..."},
						{"type": "text", "text": "The answer is 42."}
					]
				}
			}"#,
		);
		let result = parse_line(&line, "/proj", "proj").unwrap().unwrap();
		assert_eq!(result.content, "The answer is 42.");
	}

	#[test]
	fn test_skip_tool_use_blocks() {
		let line = make_line(
			r#"{
				"type": "assistant",
				"sessionId": "sess-004",
				"uuid": "msg-004",
				"timestamp": "2026-01-14T10:03:00Z",
				"gitBranch": "main",
				"message": {
					"role": "assistant",
					"content": [
						{"type": "tool_use", "id": "tool-1", "name": "bash", "input": {}},
						{"type": "text", "text": "I ran the command."}
					]
				}
			}"#,
		);
		let result = parse_line(&line, "/proj", "proj").unwrap().unwrap();
		assert_eq!(result.content, "I ran the command.");
	}

	#[test]
	fn test_skip_no_content_placeholder() {
		let line = make_line(
			r#"{
				"type": "assistant",
				"sessionId": "sess-005",
				"uuid": "msg-005",
				"timestamp": "2026-01-14T10:04:00Z",
				"gitBranch": "main",
				"message": {
					"role": "assistant",
					"content": [
						{"type": "text", "text": "(no content)"}
					]
				}
			}"#,
		);
		let result = parse_line(&line, "/proj", "proj").unwrap();
		assert!(result.is_none());
	}

	#[test]
	fn test_skip_progress_message() {
		let line = make_line(
			r#"{
				"type": "progress",
				"sessionId": "sess-006",
				"uuid": "msg-006",
				"timestamp": "2026-01-14T10:05:00Z",
				"gitBranch": "main",
				"message": {
					"role": "assistant",
					"content": "Processing..."
				}
			}"#,
		);
		let result = parse_line(&line, "/proj", "proj").unwrap();
		assert!(result.is_none());
	}

	#[test]
	fn test_skip_file_history_snapshot() {
		let line = make_line(
			r#"{
				"type": "file-history-snapshot",
				"sessionId": "sess-007",
				"uuid": "msg-007",
				"timestamp": "2026-01-14T10:06:00Z",
				"gitBranch": "main"
			}"#,
		);
		let result = parse_line(&line, "/proj", "proj").unwrap();
		assert!(result.is_none());
	}

	#[test]
	fn test_user_message_array_with_tool_results() {
		let line = make_line(
			r#"{
				"type": "user",
				"sessionId": "sess-008",
				"uuid": "msg-008",
				"timestamp": "2026-01-14T10:07:00Z",
				"gitBranch": "feature",
				"message": {
					"role": "user",
					"content": [
						{"type": "tool_result", "tool_use_id": "tool-1", "content": "output"},
						{"type": "text", "text": "Now fix the bug."}
					]
				}
			}"#,
		);
		let result = parse_line(&line, "/proj", "proj").unwrap().unwrap();
		assert_eq!(result.role, "user");
		assert_eq!(result.content, "Now fix the bug.");
	}

	#[test]
	fn test_assistant_only_tool_use_no_text() {
		let line = make_line(
			r#"{
				"type": "assistant",
				"sessionId": "sess-009",
				"uuid": "msg-009",
				"timestamp": "2026-01-14T10:08:00Z",
				"gitBranch": "main",
				"message": {
					"role": "assistant",
					"content": [
						{"type": "tool_use", "id": "tool-1", "name": "bash", "input": {}},
						{"type": "tool_use", "id": "tool-2", "name": "read", "input": {}}
					]
				}
			}"#,
		);
		let result = parse_line(&line, "/proj", "proj").unwrap();
		assert!(result.is_none());
	}
}
