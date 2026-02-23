# ccq

**Claude Code Query** — search and browse your [Claude Code](https://docs.anthropic.com/en/docs/claude-code) conversation history from the terminal.

`ccq` indexes the JSONL conversation files that Claude Code stores in `~/.claude/projects/` and provides both a CLI and an interactive TUI for searching, filtering, and reading past conversations.

![CLI demo](assets/cli-demo.gif)

![TUI demo](assets/tui-demo.gif)

## Features

- **Full-text search** powered by [Tantivy](https://github.com/quickwit-oss/tantivy) with relevance scoring
- **Interactive TUI** with three-pane layout (sessions, messages, preview)
- **Incremental indexing** — only re-indexes changed session files
- **Filter by project, branch, date range**
- **Context-aware search** — global search from sessions pane, message filter within a conversation
- **Colored CLI output** with search hit highlighting
- **JSON output** for scripting
- **Clipboard support** via OSC 52

## Installation

### Nix (flake)

```bash
nix run github:aldoborrero/ccq#ccq
```

Or add to your flake inputs:

```nix
{
  inputs.ccq.url = "github:aldoborrero/ccq";
}
```

### From source

```bash
cargo install --path .
```

## Quick start

```bash
# Build the search index (run once, then again when you want to pick up new conversations)
ccq index

# Search for something
ccq search "authentication"

# Launch the interactive TUI
ccq tui

# Or jump straight into a TUI search
ccq tui "error handling"
```

## CLI commands

### `ccq index`

Build or update the search index. Scans `~/.claude/projects/` for JSONL session files.

```bash
ccq index          # incremental update
ccq index --force  # full rebuild
```

### `ccq search <query>`

Search indexed conversations.

```bash
ccq search "nix flake"                          # grouped by session
ccq search "nix flake" -v                       # verbose (individual messages)
ccq search "bug" -p myproject                   # filter by project
ccq search "deploy" --branch main               # filter by branch
ccq search "error" --after 2025-01-01           # date range
ccq search "api" --json                         # JSON output
ccq search "refactor" -v --context 2            # show 2 messages of context around hits
ccq search "test" --limit 50                    # cap results
```

### `ccq sessions`

List and browse sessions.

```bash
ccq sessions                                    # list all sessions
ccq sessions -p myproject                       # filter by project
ccq sessions <session-id>                       # show full conversation
ccq sessions --json                             # JSON output
```

### `ccq stats`

Show index statistics.

```bash
ccq stats          # human-readable
ccq stats --json   # JSON output
```

### `ccq tui [query]`

Launch the interactive TUI browser with an optional initial search.

## TUI keybindings

| Key | Action |
|-----|--------|
| `j` / `k` | Navigate up/down |
| `Ctrl+d` / `Ctrl+u` | Half-page down/up |
| `PgDn` / `PgUp` | Half-page down/up |
| `Tab` / `Shift+Tab` | Next/previous pane |
| `Enter` | Select session / open message |
| `Esc` | Back to sessions pane |
| `/` | Search (sessions) or filter messages (messages/preview pane) |
| `n` / `N` | Next/previous search hit |
| `f` | Filter by project |
| `b` | Filter by branch |
| `m` | Toggle maximized messages view |
| `y` | Copy message to clipboard |
| `e` | Open session file in `$EDITOR` |
| `g` / `G` | Scroll preview to top/bottom |
| `?` | Show help overlay |
| `q` | Quit |

## How it works

1. `ccq index` discovers JSONL files in `~/.claude/projects/*/` and indexes each message into a local Tantivy index at `$XDG_CACHE_HOME/ccq/tantivy/`
2. Incremental indexing tracks file modification times in `meta.json` and only re-processes changed files
3. Search queries run against the Tantivy full-text index with BM25 scoring
4. The TUI loads sessions and messages from the index and provides live search with 300ms debounce

## Development

Requires Nix with flakes enabled:

```bash
# Enter dev shell
nix develop

# Build
cargo build

# Test
cargo test

# Format
nix fmt

# Lint
cargo clippy

# Nix build
nix build .#ccq
```

## License

See [LICENSE](./LICENSE) for more info.
