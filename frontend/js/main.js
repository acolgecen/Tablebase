// App orchestration for one window: wires DOM events to the API and keeps the
// UI in sync with `state`. A window is an independent environment that can hold
// several files (queried as `data`, or `data1`/`data2`/… when more than one).
// Flow: open file(s) -> preview -> (edit SQL -> run) -> paginate -> export.

import * as api from "./api.js";
import { renderGrid, renderEmpty } from "./grid.js";
import { createEditor } from "./editor.js";
import { initSplitter } from "./splitter.js";
import {
  state,
  hasFiles,
  previewSql,
  tableList,
  canGoNext,
  canGoPrev,
  beginQueryRequest,
  isCurrentQueryRequest,
  acceptEnvironment,
} from "./state.js";

// --- element handles -----------------------------------------------------
const el = (id) => document.getElementById(id);
const ui = {
  open: el("open-btn"),
  newWindow: el("newwindow-btn"),
  files: el("files"),
  run: el("run-btn"),
  reset: el("reset-btn"),
  pageInfo: el("page-info"),
  prev: el("prev-btn"),
  next: el("next-btn"),
  pageLabel: el("page-label"),
  export: el("export-btn"),
  grid: el("grid"),
  status: el("status"),
  dialog: el("reimport-dialog"),
  reimportTarget: el("reimport-target"),
  optDelimiter: el("opt-delimiter"),
  optHeader: el("opt-header"),
  welcome: el("welcome"),
  welcomeOpen: el("welcome-open"),
  welcomeNewWindow: el("welcome-newwindow"),
  welcomeQuit: el("welcome-quit"),
};

// Monokai-highlighted SQL editor (replaces the raw textarea value access).
const editor = createEditor(el("editor"), {
  onRun: () => runQuery(editor.getValue(), 0, false),
});
initSplitter(el("splitter"));

// The file whose "Import options" dialog is currently open.
let reimportPath = null;

// --- status helpers ------------------------------------------------------
function setStatus(text, kind = "") {
  ui.status.textContent = text;
  ui.status.className = `status ${kind}`.trim();
}
function setBusy(text) {
  ui.status.textContent = text;
  ui.status.className = "status busy";
}
function showError(err) {
  // Backend errors are { kind, message }; fall back to string form otherwise.
  const msg = err && err.message ? `${err.kind}: ${err.message}` : String(err);
  setStatus(msg, "error");
}

// --- environment rendering ----------------------------------------------
// Render the window's open-files bar and reconcile dependent UI. This is the
// single place the file chips and empty/welcome states are derived from `state`.
function renderEnv(info) {
  if (!acceptEnvironment(info)) return false;
  ui.welcome.hidden = hasFiles();

  ui.files.replaceChildren();
  for (const file of info.files) {
    ui.files.appendChild(renderChip(file));
  }

  const enabled = hasFiles();
  ui.run.disabled = !enabled;
  ui.reset.disabled = !enabled;
  if (!enabled) {
    // No files: nothing to query, so clear results and the editor helper.
    setResultActionsEnabled(false);
    renderEmpty(ui.grid, "Open a CSV or TSV file to get started.");
  }
  return true;
}

function renderChip(file) {
  const chip = document.createElement("span");
  chip.className = "file-chip";

  const name = document.createElement("span");
  name.className = "file-chip-name";
  name.textContent = file.file_name;
  name.title = `${file.source_path}\n${file.row_count.toLocaleString()} rows × ${file.columns.length} cols${file.cached ? " (cached)" : ""}`;

  const table = document.createElement("span");
  table.className = "file-chip-table";
  table.textContent = file.table;

  const opts = iconButton("i-options", "Import options…", "opts");
  opts.dataset.path = file.source_path;
  const close = iconButton("i-close", `Close ${file.file_name}`, "close");
  close.dataset.path = file.source_path;

  chip.append(name, table, opts, close);
  return chip;
}

function iconButton(symbol, title, kind) {
  const btn = document.createElement("button");
  btn.className = `file-chip-btn ${kind}`;
  btn.title = title;
  btn.innerHTML = `<svg class="ico"><use href="#${symbol}" /></svg>`;
  return btn;
}

// --- open / close / reimport --------------------------------------------
async function addFiles() {
  setBusy("Opening file(s)");
  try {
    const before = state.env.files.length;
    const info = await api.addFiles();
    if (!renderEnv(info)) return;
    const added = info.files.length - before;
    if (added <= 0) return setStatus("No file added.");

    if (before === 0) {
      // First file(s) in this window: preview the first table straight away.
      const sql = previewSql();
      state.autoSql = sql;
      editor.setValue(sql);
      runQuery(sql, 0, false);
    } else {
      // Files were added alongside existing ones — the table names shifted to
      // data1/data2/…, so surface the new set instead of clobbering their SQL.
      setStatus(`Added ${added} file(s). Tables: ${tableList()}.`);
    }
  } catch (err) {
    showError(err);
    syncEnv();
  }
}

async function closeFile(path) {
  setBusy("Closing file");
  try {
    const info = await api.closeFile(path);
    if (!renderEnv(info)) return;
    if (!hasFiles()) {
      clearResults();
      setStatus("All files closed.");
    } else {
      setStatus(`Closed file. Tables: ${tableList()}.`);
    }
  } catch (err) {
    showError(err);
    syncEnv();
  }
}

function openReimport(path) {
  reimportPath = path;
  const file = state.env.files.find((f) => f.source_path === path);
  ui.reimportTarget.textContent = file ? `"${file.file_name}"` : "the file";
  ui.dialog.showModal();
}

async function applyReimport() {
  if (!reimportPath) return;
  const delimiter = ui.optDelimiter.value || null;
  const headerRaw = ui.optHeader.value;
  const header = headerRaw === "" ? null : headerRaw === "true";
  setBusy("Re-importing");
  try {
    const info = await api.reimport(reimportPath, { delimiter, header });
    if (!renderEnv(info)) return;
    setStatus(`Re-imported. Tables: ${tableList()}.`);
  } catch (err) {
    showError(err);
    syncEnv();
  } finally {
    reimportPath = null;
  }
}

// Pull the authoritative environment snapshot from the backend (used to
// resynchronise the UI after an error left it uncertain).
async function syncEnv() {
  try {
    renderEnv(await api.envInfo());
  } catch {
    // Best-effort: leave the UI as-is if even the snapshot fails.
  }
}

// --- query + pagination --------------------------------------------------
async function runQuery(sql, page, reuseSession) {
  if (!hasFiles()) return setStatus("Open a file first.");
  const requestId = beginQueryRequest();
  const previousQueryId = state.queryId;
  setBusy("Running query");
  try {
    const response = reuseSession && state.queryId !== null
      ? await api.fetchPage(state.queryId, page, state.pageSize)
      : await api.startQuery(sql, state.pageSize);
    if (!isCurrentQueryRequest(requestId)) {
      if (!reuseSession) api.cancelQuery(response.query_id).catch(() => {});
      return;
    }
    const result = response.result;
    state.sql = sql;
    state.page = page;
    state.lastResult = result;
    state.queryId = response.query_id;
    state.queryRevision = response.workspace_revision;
    state.totalRows = null; // reset; counted lazily below
    renderGrid(ui.grid, result, page * state.pageSize);
    setResultActionsEnabled(true);
    updatePager();
    setStatus(`${result.returned.toLocaleString()} rows on this page.`);
    if (!reuseSession && previousQueryId !== null && previousQueryId !== state.queryId) {
      api.cancelQuery(previousQueryId).catch(() => {});
    }
    refreshCount(state.queryId, state.queryRevision);
  } catch (err) {
    if (isCurrentQueryRequest(requestId)) showError(err);
  }
}

// Count the full result lazily so the first page paints immediately.
async function refreshCount(queryId, workspaceRevision) {
  try {
    const count = await api.countQuery(queryId);
    if (
      state.queryId !== count.query_id ||
      state.queryRevision !== workspaceRevision ||
      count.workspace_revision !== workspaceRevision
    ) return;
    state.totalRows = count.total_rows;
    updatePager();
  } catch {
    // Non-fatal: leave totals unknown, pagination still works heuristically.
  }
}

function updatePager() {
  ui.prev.disabled = !canGoPrev();
  ui.next.disabled = !canGoNext();
  const pageHuman = state.page + 1;
  if (state.totalRows !== null) {
    const pages = Math.max(1, Math.ceil(state.totalRows / state.pageSize));
    ui.pageLabel.textContent = `Page ${pageHuman} / ${pages}`;
    ui.pageInfo.textContent = `${state.totalRows.toLocaleString()} rows total`;
  } else {
    ui.pageLabel.textContent = `Page ${pageHuman}`;
    ui.pageInfo.textContent = "Counting…";
  }
}

// --- export --------------------------------------------------------------
async function exportResults() {
  if (!state.lastResult) return;
  setBusy("Exporting");
  try {
    const info = await api.exportResults(state.queryId);
    if (!info) return setStatus("Export cancelled.");
    setStatus(`Saved ${info.format} to ${info.path}`);
  } catch (err) {
    showError(err);
  }
}

function clearResults() {
  state.lastResult = null;
  state.totalRows = null;
  if (state.queryId !== null) api.cancelQuery(state.queryId).catch(() => {});
  state.queryId = null;
  state.queryRevision = null;
  renderEmpty(ui.grid, "Open a CSV or TSV file to get started.");
  setResultActionsEnabled(false);
  ui.pageInfo.textContent = "—";
  ui.pageLabel.textContent = "Page 1";
}

function setResultActionsEnabled(enabled) {
  ui.export.disabled = !enabled;
  if (!enabled) {
    ui.prev.disabled = true;
    ui.next.disabled = true;
  }
}

// --- event wiring --------------------------------------------------------
ui.open.addEventListener("click", addFiles);
ui.newWindow.addEventListener("click", () => api.newWindow().catch(showError));
ui.welcomeOpen.addEventListener("click", addFiles);
ui.welcomeNewWindow.addEventListener("click", () => api.newWindow().catch(showError));
ui.welcomeQuit.addEventListener("click", () => api.quit());
ui.run.addEventListener("click", () => runQuery(editor.getValue(), 0, false));
ui.reset.addEventListener("click", () => {
  const sql = previewSql();
  state.autoSql = sql;
  editor.setValue(sql);
  runQuery(sql, 0, false);
});
ui.prev.addEventListener("click", () => runQuery(state.sql, state.page - 1, true));
ui.next.addEventListener("click", () => runQuery(state.sql, state.page + 1, true));
ui.export.addEventListener("click", exportResults);

// Per-file chip actions (event delegation: chips are re-rendered on every change).
ui.files.addEventListener("click", (e) => {
  const btn = e.target.closest(".file-chip-btn");
  if (!btn) return;
  if (btn.classList.contains("close")) closeFile(btn.dataset.path);
  else if (btn.classList.contains("opts")) openReimport(btn.dataset.path);
});

ui.dialog.addEventListener("close", () => {
  if (ui.dialog.returnValue === "apply") applyReimport();
  else reimportPath = null;
});

// Cmd/Ctrl+Enter to run is handled inside the editor (see editor.js).
// Sync from the backend so a reloaded window restores its open files.
syncEnv();
setStatus("Ready. Open a CSV or TSV file to begin.");
