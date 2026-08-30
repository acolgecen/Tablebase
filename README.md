# Tablebase

Tablebase is a macOS desktop app for exploring CSV, TSV, and other delimited text files with read-only SQL. It uses DuckDB for ingestion and queries, presents large results in pages, and exports complete result sets to CSV.

Each window is an independent workspace. Files are shared by the query tabs in that window, while every tab owns its SQL draft, result, pagination state, count, and immutable backend query session. Switching tabs never discards a result; rerunning or closing that tab releases its previous session.

## Highlights

- Open several CSV, TSV, or TXT files and query or join them in one workspace.
- Run independent queries concurrently in multiple tabs.
- Switch tabs without losing SQL drafts, results, pagination, or export access.
- Keep existing tab results available after their source file is removed from the workspace.
- Create additional windows with fully isolated files and query tabs.
- Stream source data into DuckDB instead of loading the entire file into application memory.
- Auto-detect delimiters, headers, and column types with DuckDB `read_csv`.
- Override delimiter, header detection, and SQL table names per file.
- Browse results in fixed 2,000-row pages with a lazily calculated total count.
- Export the complete, unpaginated result as a headered CSV file.
- Use a lightweight syntax-highlighted SQL editor without a frontend framework or bundler.

## Using Tablebase

### Workspaces and windows

A Tablebase window is one workspace. Its left sidebar contains the open files, and its top tab strip contains the queries that use those files.

Create another independent workspace with **File → New Window** or `⌘N`. Opening or closing files in one window never changes another window.

The welcome screen provides actions to open files or quit. Both the welcome-screen Quit action and `⌘Q` ask for confirmation before quitting Tablebase and closing every open window, query tab, and retained result.

### Open and name files

Select **Add files…** in the left sidebar to open the native multi-file picker. Tablebase accepts `.csv`, `.tsv`, and `.txt` files.

Opened files become SQL views with stable abbreviations:

- The first file is named `data` automatically.
- Additional files are named `data1`, `data2`, and so on.
- The configuration button on a file card can assign a custom abbreviation.

The first group of files is previewed automatically with:

```sql
SELECT * FROM data
```

Each file card shows its source name and SQL abbreviation, with actions to configure or close the file. Existing abbreviations remain stable when files are added or removed; newly added files receive the next automatic `dataN` name in the workspace's sequence.

Removing a file prevents new queries from using it. Results already retained by query tabs continue to work because each completed query owns an immutable snapshot of its workspace.

### Configure imports

Imports use DuckDB auto-detection by default. The per-file configuration dialog supports:

- **SQL abbreviation:** a unique view name used in queries.
- **Delimiter:** auto-detect, comma, tab, semicolon, or pipe.
- **Header row:** auto-detect, present, or absent.

Abbreviations may contain letters, numbers, and underscores but cannot begin with a number. Tablebase rejects DuckDB-reserved words and names already used by another file in the same workspace.

Applying a configuration change switches the file to the immutable cache artifact produced for those options. Existing query tabs keep their previous snapshots; new queries use the updated file configuration.

### Query tabs

Use the **+** button or `⌘T` to create a query tab. Every tab keeps independent:

- Editor text.
- In-flight request state.
- Query result and total count.
- Current result page.
- CSV export session.

Tabs can run queries concurrently. A slow query in one tab does not replace or invalidate the result shown in another.

Close the active tab with its close button, **File → Close Query Tab**, or `⌘W`. Closing a tab cancels pending work and releases only that tab's backend session. Other tabs are unaffected. Closing the final tab creates a new empty query tab so the window remains usable.

### Run and paginate queries

Enter SQL in the editor and choose **Run**, or press `⌘Return`. User SQL must contain exactly one read-only query. Tablebase accepts `SELECT` and `WITH … SELECT` queries and rejects writes or administrative statements such as `INSERT`, `UPDATE`, `DELETE`, `DROP`, `COPY`, `PRAGMA`, and `ATTACH`.

Results are displayed 2,000 rows at a time with `LIMIT` and `OFFSET`. The first page renders before Tablebase runs a separate full count, allowing it to show the total rows and number of pages without delaying the initial result.

All non-null cell values are serialized as text by DuckDB. This preserves values such as dates and large decimals without relying on JavaScript number precision.

### Export results

**Save CSV** exports the complete query result, not only the visible page. Export uses DuckDB `COPY` with a header row. TSV, Parquet, and other export formats are not currently supported.

### Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `⌘Return` | Run the active query |
| `⌘T` | Create a query tab |
| `⌘W` | Close the active query tab |
| `⌘N` | Create a new independent window |
| `⌘Q` | Ask for confirmation, then quit Tablebase and close all windows |
| `Tab` | Insert two spaces in the SQL editor |

`Ctrl+Enter`, `Ctrl+T`, `Ctrl+W`, `Ctrl+N`, and `Ctrl+Q` are the corresponding cross-platform accelerators used by the implementation.

## Read-only safety model

Tablebase protects source data in two complementary ways:

1. `guardrail.rs` parses SQL with `sqlparser`'s PostgreSQL dialect, requires exactly one statement, and accepts only statements represented as read-only queries.
2. Every query session receives an isolated DuckDB catalog whose cached source databases are attached with `READ_ONLY`.

The in-memory connection remains writable only so Tablebase can create the `data` and `dataN` views. User queries run after guardrail validation, and Tablebase never modifies or deletes source files.

DuckDB executes the query, while the guardrail's PostgreSQL parser decides which SQL is accepted. Queries therefore need to be valid for both layers.

## Result and session lifetime

Query results are intentionally scoped to their tabs:

- Rerunning a tab replaces and releases that tab's previous session only after the new query succeeds.
- An invalid rerun leaves the previous result available.
- Switching tabs preserves each tab's editor and displayed page.
- Closing a source file does not destroy existing query snapshots.
- Closing a tab cancels its work and releases its session.
- Closing a window releases every query session and file lease owned by that window.
- Confirmed Quit closes all windows and releases all in-memory query state.

Tabs and query history are not restored after the application exits.

## Architecture

```text
Tablebase application
  │
  ├─ Native menu
  │    ├─ New Window                  ⌘N
  │    ├─ Close Query Tab             ⌘W
  │    └─ Confirmed Quit              ⌘Q
  │
  └─ macOS window / independent workspace
       │
       ├─ Left file sidebar
       │    └─ CSV, TSV, or TXT
       │         └─ DuckDB read_csv auto-detection
       │              └─ immutable cached database
       │
       ├─ Revisioned file registry
       │    └─ expose views as data, data1, data2, …
       │
       ├─ Query tab 1
       │    └─ validated, immutable query session
       │         ├─ paged result
       │         ├─ independent total count
       │         └─ full-result CSV export
       │
       ├─ Query tab 2
       │    └─ independent query session and retained result
       │
       └─ Query tab N
            └─ independent query session and retained result
```

### Backend (`src-tauri/src/`)

| File | Responsibility |
| --- | --- |
| `main.rs` | Tauri setup, native menus and accelerators, focused-window tab events, and window cleanup |
| `commands.rs` | Tauri adapters, native file dialogs, confirmed quit flow, and runtime window creation |
| `application.rs` | UI-independent import, workspace, query, pagination, cancellation, and export use cases |
| `model.rs` | Serializable data transferred between Rust and the frontend |
| `error.rs` | Unified `AppError` and `AppResult` types |
| `state.rs` | Per-window `Environment` map, shared query manager, and application defaults |
| `cache.rs` | Atomic, option-aware cache materialization, leases, and LRU eviction |
| `ingest.rs` | Delimited-file ingestion into a persistent DuckDB table |
| `guardrail.rs` | Single-statement, read-only SQL validation |
| `query.rs` | Query execution, pagination, counting, and text serialization |
| `query_session.rs` | Persistent immutable query snapshots, independent jobs, and explicit cancellation |
| `export.rs` | Full-result CSV export through DuckDB `COPY` |
| `environment.rs` | Revisioned file sets, transactional mutation, and immutable query plans |

### Frontend (`frontend/`)

| File | Responsibility |
| --- | --- |
| `index.html` | Welcome screen, query tabs, file sidebar, SQL editor, result controls, and import dialog |
| `styles.css` | Two-column workspace, tab strip, result grid, and visual styling |
| `js/api.js` | Contract-checked wrappers around Tauri commands |
| `js/contracts.js` | Runtime verification of Rust response shapes |
| `js/state.js` | Shared window environment and independent query-tab state |
| `js/editor.js` | SQL tokenization, syntax highlighting, caret restoration, and editor shortcuts |
| `js/splitter.js` | Draggable editor/result divider |
| `js/grid.js` | `PageResult` table rendering |
| `js/main.js` | Tab lifecycle, concurrent request coordination, file actions, pagination, and export flow |

The frontend is static HTML, CSS, and ES modules. Its editor uses one `contenteditable="plaintext-only"` element, retokenizes after edits, and restores the caret by character offset. Tablebase does not use CodeMirror, a frontend framework, or a bundler.

## Cache

Cached databases are stored at:

```text
~/Library/Application Support/Tablebase/cache/
```

A cache key contains the canonical source path, precise file metadata, import options, and a cache schema version. Changing any of those values creates a new immutable entry. Cache creation is serialized per source, installed atomically, and leased while a workspace or query session uses it.

The cache is capped at 8 GiB by default. When it exceeds that limit, Tablebase removes the least-recently accessed databases that are not leased by a window or retained query tab. Source files are never deleted or modified.

## Development

### Prerequisites

- macOS.
- Stable Rust toolchain.
- Xcode Command Line Tools.
- Node.js only when running the frontend unit tests.

Node.js is not required to build or run the application; Tauri serves the static frontend files directly.

### Run the app

```bash
cd src-tauri
cargo run
```

The first build is slow because the bundled DuckDB C++ library and Rust dependency tree must be compiled. Later builds reuse Cargo's build cache.

### Run the tests

```bash
cd src-tauri
cargo test

cd ../frontend
npm test
```

The Rust suite covers SQL guardrails, concurrent and option-aware caching, transactional workspace changes, multi-file joins, query-session lifetime, pagination, cancellation boundaries, and CSV export. The frontend suite covers command contracts, monotonic workspace updates, independent tab state, request generations, and tab-close isolation. Tests use temporary cache directories and never touch the production application cache.

## macOS release build

`build/build.sh` builds a macOS `.app`, optionally regenerates its icon, signs it, verifies the signature, and copies it to `dist/`.

Install the Tauri CLI first:

```bash
cargo install tauri-cli --locked
```

Common commands from the repository root:

```bash
build/build.sh                         # build with ad-hoc signing
build/build.sh --no-sign               # unsigned local build
build/build.sh --icon path/to/icon.png # regenerate src-tauri/icons/ first
build/build.sh --skip-build            # repackage/sign an existing bundle
```

For Developer ID signing:

```bash
SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)" build/build.sh
```

The script uses `sccache` automatically when available; set `NO_SCCACHE=1` to disable it. Run `build/build.sh --help` for all supported flags and environment variables.

The current bundle target is `.app` only. The script does not create a DMG or notarize the application. Ad-hoc and unsigned builds may trigger Gatekeeper warnings on other Macs.

## Current limitations

- Input is limited to delimited text files handled by DuckDB `read_csv`; Parquet, JSON, and Excel are not supported.
- Export is CSV-only.
- Page size is fixed at 2,000 rows and pagination uses `LIMIT`/`OFFSET`.
- Calculating the total result count can require a second full scan.
- The UI does not expose a manual cancel button or determinate progress for queries and ingestion; closing a query tab still cancels that tab's active work.
- Existing SQL is not rewritten when files are added, removed, renamed, or reconfigured.
- Tabs, query history, and results are not persisted across application launches.
- Finder **Open With**, saved tables, and editor autocomplete are not implemented.
