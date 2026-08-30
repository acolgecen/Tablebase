# Tablebase

Tablebase is a macOS desktop app for exploring delimited text files with read-only SQL. Open one or more CSV, TSV, or TXT files, query them together with DuckDB, page through the results, and export the full result set as CSV.

Each window is an independent workspace with its own files, SQL editor, and query results. Ingested data is cached on disk, so reopening an unchanged file with the same import options can reuse its existing DuckDB database.

## Highlights

- Open several files at once and query or join them in one window.
- Create additional windows with fully independent workspaces.
- Stream files into DuckDB instead of loading the entire source into application memory.
- Auto-detect delimiters, headers, and column types through DuckDB `read_csv`.
- Override delimiter and header detection when re-importing a file.
- Run a single read-only `SELECT` or `WITH` query at a time.
- Browse results in 2,000-row pages with a lazily calculated total row count.
- Export the complete, unpaginated query result as a headered CSV file.
- Use a lightweight, Monokai-highlighted SQL editor with no frontend framework or bundler.

## Using the app

### Open and name files

The welcome screen provides actions to open files, create another window, or quit. The native file picker supports multi-selection and accepts `.csv`, `.tsv`, and `.txt` files.

Tablebase exposes opened files as SQL views with stable abbreviations:

- The first file is named `data` automatically.
- Additional files are named `data1`, `data2`, … automatically.
- Use a file chip's configuration button to choose a custom abbreviation.

The first set of opened files is previewed automatically with `SELECT * FROM data`. Each file chip shows its current SQL abbreviation and provides actions to configure or close the file.

Existing abbreviations remain stable when files are added or closed. Newly added files receive the next automatic `dataN` abbreviation.

### File configuration

Imports use DuckDB's auto-detection by default. The per-file configuration dialog can set:

- **SQL abbreviation:** a unique identifier used in queries
- **Delimiter:** comma, tab, semicolon, or pipe
- **Header row:** auto-detect, present, or absent

Abbreviations use letters, numbers, and underscores and cannot start with a number. DuckDB-reserved SQL words and abbreviations already used by another open file are rejected. Applying import changes switches that source to the immutable cache artifact produced by the selected options.

### Query and paginate

User SQL is parsed before execution and must contain exactly one read-only query. `SELECT` statements and `WITH … SELECT` common table expressions are accepted; writes and administrative statements such as `INSERT`, `UPDATE`, `DELETE`, `DROP`, `COPY`, `PRAGMA`, and `ATTACH` are rejected.

Results are displayed 2,000 rows at a time using `LIMIT`/`OFFSET`. After the first page renders, Tablebase runs a separate count of the full query so it can show the total number of rows and pages.

All non-null grid values are serialized as text by DuckDB, preserving values such as dates and large decimals without relying on JavaScript number precision.

### Export

**Save CSV** exports the complete query result, not only the visible page. Export uses DuckDB `COPY` with a header row. TSV, Parquet, and other export formats are not currently supported.

### Keyboard shortcuts

| Shortcut | Action |
| --- | --- |
| `⌘` + `Return` on macOS | Run the current query |
| `Ctrl` + `Enter` | Run the current query |
| `Tab` | Insert two spaces in the SQL editor |

## Read-only model

Tablebase protects source data in two complementary ways:

1. `guardrail.rs` parses SQL with `sqlparser`'s PostgreSQL dialect, requires exactly one statement, and accepts only statements represented as read-only queries.
2. Each query session receives an isolated DuckDB catalog whose cached source databases are attached with `READ_ONLY`.

The in-memory connection itself remains writable so Tablebase can create and rebuild the `data`/`dataN` views. User queries run only after guardrail validation, and source files are never modified.

DuckDB executes the query, while the guardrail's PostgreSQL parser determines which SQL is accepted. Queries therefore need to be valid for both layers.

## Architecture

```text
macOS window / independent workspace
  │
  ├─ Open CSV, TSV, or TXT
  │    └─ DuckDB read_csv auto-detection
  │         └─ ~/Library/Application Support/Tablebase/cache/<key>.duckdb
  │
  ├─ Revisioned dataset registry
  │    └─ expose views as data or data1, data2, …
  │
  ├─ validate one read-only query
  │    └─ immutable, cancellable query session
  │         ├─ render a 2,000-row page
  │         └─ independently count the full result
  │
  └─ export the full result to CSV
```

### Backend (`src-tauri/src/`)

| File | Responsibility |
| --- | --- |
| `main.rs` | Tauri setup, plugin registration, commands, and window cleanup |
| `commands.rs` | Thin Tauri adapters and native open/save dialogs |
| `application.rs` | UI-independent import, workspace, query, and export use cases |
| `model.rs` | Serializable data transferred between Rust and the frontend |
| `error.rs` | Unified `AppError` and `AppResult` types |
| `state.rs` | Per-window `Environment` map and application defaults |
| `cache.rs` | Atomic, option-aware cache materialization, leases, and LRU eviction |
| `ingest.rs` | Delimited-file ingestion into a persistent DuckDB table |
| `guardrail.rs` | Single-statement, read-only SQL validation |
| `query.rs` | Query description, pagination, counting, and text serialization |
| `query_session.rs` | Immutable query snapshots, independent jobs, and cancellation |
| `export.rs` | Full-result CSV export through DuckDB `COPY` |
| `environment.rs` | Revisioned datasets, transactional mutation, and query plans |

### Frontend (`frontend/`)

| File | Responsibility |
| --- | --- |
| `index.html` | Welcome screen, toolbars, editor, results grid, and import dialog |
| `styles.css` | Application layout and visual styling |
| `js/api.js` | Wrappers around Tauri `invoke` |
| `js/contracts.js` | Runtime verification of Rust command response shapes |
| `js/state.js` | Per-window UI state and pagination predicates |
| `js/editor.js` | SQL tokenization, syntax highlighting, caret restoration, and shortcuts |
| `js/splitter.js` | Draggable editor/results divider |
| `js/grid.js` | `PageResult` table rendering |
| `js/main.js` | UI event handling and application flow |

The frontend is static HTML, CSS, and ES modules. The editor uses one `contenteditable="plaintext-only"` element, retokenizes its contents after edits, and restores the caret by character offset. There is no CodeMirror, frontend framework, Node runtime, bundler, or `node_modules` directory.

## Cache

Cached databases are stored at:

```text
~/Library/Application Support/Tablebase/cache/
```

A cache key is derived from the canonical source path, precise file metadata, import options, and a cache schema version. Changing any of those values creates a new immutable entry. Cache creation is serialized per source, installed atomically, and leased while a workspace or query session uses it.

The cache is capped at 8 GiB by default. When it exceeds the cap, Tablebase removes the least-recently accessed unleased databases until the cache is under the limit. Source files are never deleted or modified.

## Development

### Prerequisites

- macOS
- Stable Rust toolchain
- Xcode Command Line Tools

Node is not required to build or run the app because the frontend is served as static files.

### Run and test

```bash
cd src-tauri
cargo run
cargo test
```

The first build is slow because the bundled DuckDB C++ library and the Rust dependency tree must be compiled. Later builds reuse Cargo's build cache.

The test suite covers SQL guardrails, concurrent and option-aware caching, transactional workspace changes, multi-file joins, query sessions, pagination, cancellation boundaries, and CSV export. Tests use isolated temporary cache directories and never touch the production application cache.

## macOS release build

`build/build.sh` builds a macOS `.app`, optionally regenerates its icon, signs it, verifies the signature, and copies it to `dist/`.

Install the Tauri CLI first:

```bash
cargo install tauri-cli --locked
```

Common build commands:

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

The current bundle target is `.app` only. The script does **not** create a DMG or notarize the application. Ad-hoc and unsigned builds may trigger Gatekeeper warnings on other Macs.

## Current limitations

- Input is limited to delimited text files handled by DuckDB `read_csv`; Parquet, JSON, and Excel are not supported as sources.
- Export is CSV-only.
- Page size is fixed at 2,000 rows and pagination uses `LIMIT`/`OFFSET`.
- Calculating the total result count can require a second full scan.
- Queries and ingestion do not currently expose cancellation or determinate progress.
- Table view names may change when files are added or closed, and existing SQL is not rewritten.
- Finder **Open With**, query history, saved tables, and editor autocomplete are not implemented.
