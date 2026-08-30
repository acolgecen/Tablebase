// Thin wrappers over the Rust commands. We use Tauri's invoke primitive
// directly, so the frontend needs no npm packages or bundler and the complete
// global Tauri API remains disabled.
//
// Tauri maps camelCase argument keys here to the snake_case Rust parameters,
// and automatically tags each call with the *calling window*, so the backend
// resolves the right per-window environment without us passing a label.

import * as contract from "./contracts.js";

// Tablebase needs only the invoke primitive. `withGlobalTauri` is disabled, so
// the complete public Tauri API is not injected into the page.
const invoke = window.__TAURI_INTERNALS__.invoke;

/** Open a brand-new, independent window with its own empty environment. */
export function newWindow() {
  return invoke("new_window");
}

/** Open a native (multi-select) picker and add the files to this window. Resolves to EnvironmentInfo. */
export function addFiles() {
  return invoke("add_files").then(contract.environmentInfo);
}

/** Close one file in this window. Resolves to the updated EnvironmentInfo. */
export function closeFile(path) {
  return invoke("close_file", { path }).then(contract.environmentInfo);
}

/** Re-ingest one file with explicit { delimiter, header } options. Resolves to EnvironmentInfo. */
export function reimport(path, options) {
  return invoke("reimport", { path, options }).then(contract.environmentInfo);
}

/** Current environment snapshot for this window. Resolves to EnvironmentInfo. */
export function envInfo() {
  return invoke("env_info").then(contract.environmentInfo);
}

/** Start an immutable query session and return its first page. */
export function startQuery(sql, pageSize) {
  return invoke("start_query", { sql, pageSize }).then(contract.queryPage);
}

/** Fetch one page from an existing immutable query session. */
export function fetchPage(queryId, page, pageSize) {
  return invoke("fetch_page", { queryId, page, pageSize }).then(contract.queryPage);
}

/** Total row count of a query session (full scan). */
export function countQuery(queryId) {
  return invoke("count_query", { queryId }).then(contract.queryCount);
}

/** Interrupt and release a superseded query session. */
export function cancelQuery(queryId) {
  return invoke("cancel_query", { queryId });
}

/** Export a query's full result set. Resolves to ExportInfo or null (cancelled). */
export function exportResults(queryId) {
  return invoke("export_results", { queryId }).then(contract.optionalExportInfo);
}

/** Quit the application. */
export function quit() {
  return invoke("quit");
}
