// One window owns a shared file environment and multiple independent query
// tabs. Each tab retains its SQL, page, result, count, and immutable backend
// session until the tab is closed.

import * as api from "./api.js";
import { renderGrid, renderEmpty } from "./grid.js";
import { createEditor } from "./editor.js";
import { initSplitter } from "./splitter.js";
import {
  state,
  activeTab,
  queryTab,
  createQueryTab,
  selectQueryTab,
  closeQueryTab,
  hasFiles,
  hasPersistedResults,
  previewSql,
  tableList,
  canGoNext,
  canGoPrev,
  beginQueryRequest,
  isCurrentQueryRequest,
  acceptEnvironment,
} from "./state.js";

const el = (id) => document.getElementById(id);
const ui = {
  open: el("open-btn"),
  files: el("files"),
  fileCount: el("file-count"),
  tabs: el("query-tabs"),
  newTab: el("new-tab-btn"),
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
  optAbbreviation: el("opt-abbreviation"),
  welcome: el("welcome"),
  welcomeOpen: el("welcome-open"),
  welcomeQuit: el("welcome-quit"),
};

const editor = createEditor(el("editor"), {
  onRun: () => {
    const tab = activeTab();
    if (tab) runQuery(tab, editor.getValue(), 0, false);
  },
  onChange: (sql) => {
    const tab = activeTab();
    if (tab) tab.draftSql = sql;
  },
});
initSplitter(el("splitter"));

let reimportPath = null;

function errorMessage(err) {
  return err && err.message ? `${err.kind}: ${err.message}` : String(err);
}

function setTabStatus(tab, text, kind = "") {
  tab.status = text;
  tab.statusKind = kind;
  tab.busy = kind === "busy";
  if (tab === activeTab()) renderStatus(tab);
  renderTabs();
}

function renderStatus(tab) {
  ui.status.textContent = tab.status;
  ui.status.className = `status ${tab.statusKind}`.trim();
}

function showError(tab, err) {
  setTabStatus(tab, errorMessage(err), "error");
}

function renderTabs() {
  ui.tabs.replaceChildren();
  for (const tab of state.tabs) {
    const item = document.createElement("div");
    item.className = "query-tab";
    if (tab.id === state.activeTabId) item.classList.add("active");
    if (tab.lastResult) item.classList.add("has-results");
    if (tab.busy) item.classList.add("busy");
    item.dataset.tabId = tab.id;
    item.setAttribute("role", "tab");
    item.setAttribute("aria-selected", String(tab.id === state.activeTabId));
    item.tabIndex = tab.id === state.activeTabId ? 0 : -1;

    const stateDot = document.createElement("span");
    stateDot.className = "query-tab-state";
    const label = document.createElement("span");
    label.className = "query-tab-label";
    label.textContent = tab.title;
    const close = document.createElement("button");
    close.className = "query-tab-close";
    close.dataset.closeTab = tab.id;
    close.title = `Close ${tab.title}`;
    close.setAttribute("aria-label", `Close ${tab.title}`);
    close.innerHTML = '<svg class="ico"><use href="#i-close" /></svg>';
    item.append(stateDot, label, close);
    ui.tabs.appendChild(item);
  }
}

function renderActiveTab({ focus = false } = {}) {
  const tab = activeTab();
  if (!tab) return;
  editor.setValue(tab.draftSql);
  ui.run.disabled = !hasFiles();
  ui.reset.disabled = !hasFiles();

  if (tab.lastResult) {
    renderGrid(ui.grid, tab.lastResult, tab.page * state.pageSize);
    ui.export.disabled = tab.queryId === null;
  } else {
    renderEmpty(
      ui.grid,
      hasFiles()
        ? "Write a query and press ⌘↵ to run it."
        : "Open a CSV or TSV file to get started.",
    );
    ui.export.disabled = true;
  }
  updatePager(tab);
  renderStatus(tab);
  if (focus) editor.focus();
}

function updateWelcome() {
  // Removing a source must not cover results already retained by a query tab.
  ui.welcome.hidden = hasFiles() || hasPersistedResults();
}

function activateTab(id, focus = false) {
  const current = activeTab();
  if (current) current.draftSql = editor.getValue();
  if (!selectQueryTab(id)) return;
  renderTabs();
  renderActiveTab({ focus });
}

function addQueryTab({ sql = "", focus = true } = {}) {
  const current = activeTab();
  if (current) current.draftSql = editor.getValue();
  const tab = createQueryTab(sql);
  renderTabs();
  renderActiveTab({ focus });
  return tab;
}

function removeQueryTab(id) {
  const wasActive = state.activeTabId === id;
  const tab = queryTab(id);
  if (!tab) return;
  tab.activeQueryRequest += 1;
  if (tab.queryId !== null) api.cancelQuery(tab.queryId).catch(() => {});
  closeQueryTab(id);
  if (state.tabs.length === 0) createQueryTab();
  renderTabs();
  if (wasActive) renderActiveTab({ focus: true });
  updateWelcome();
}

// --- environment rendering ----------------------------------------------
function renderEnv(info) {
  if (!acceptEnvironment(info)) return false;
  ui.files.replaceChildren();
  for (const file of info.files) ui.files.appendChild(renderFileCard(file));
  ui.fileCount.textContent = String(info.files.length);
  ui.run.disabled = !hasFiles();
  ui.reset.disabled = !hasFiles();
  updateWelcome();
  if (!activeTab()?.lastResult) renderActiveTab();
  return true;
}

function renderFileCard(file) {
  const card = document.createElement("div");
  card.className = "file-chip";

  const name = document.createElement("span");
  name.className = "file-chip-name";
  name.textContent = file.file_name;
  name.title = `${file.source_path}\n${file.row_count.toLocaleString()} rows × ${file.columns.length} cols${file.cached ? " (cached)" : ""}`;

  const table = document.createElement("span");
  table.className = "file-chip-table";
  table.textContent = file.table;
  table.title = `SQL table: ${file.table}`;

  const opts = iconButton("i-options", "File configuration…", "opts");
  opts.dataset.path = file.source_path;
  const close = iconButton("i-close", `Close ${file.file_name}`, "close");
  close.dataset.path = file.source_path;
  card.append(name, table, opts, close);
  return card;
}

function iconButton(symbol, title, kind) {
  const button = document.createElement("button");
  button.className = `file-chip-btn ${kind}`;
  button.title = title;
  button.setAttribute("aria-label", title);
  button.innerHTML = `<svg class="ico"><use href="#${symbol}" /></svg>`;
  return button;
}

async function addFiles() {
  const tab = activeTab();
  if (tab) setTabStatus(tab, "Opening file(s)", "busy");
  try {
    const before = state.env.files.length;
    const info = await api.addFiles();
    if (!renderEnv(info)) return;
    const added = info.files.length - before;
    if (added <= 0) {
      if (tab) setTabStatus(tab, "No file added.");
      return;
    }

    if (before === 0) {
      let target = activeTab();
      if (target.lastResult || target.queryId !== null || target.draftSql.trim()) {
        target = addQueryTab({ focus: false });
      }
      const sql = previewSql();
      target.autoSql = sql;
      target.draftSql = sql;
      if (target === activeTab()) editor.setValue(sql);
      runQuery(target, sql, 0, false);
    } else {
      setTabStatus(activeTab(), `Added ${added} file(s). Tables: ${tableList()}.`);
    }
  } catch (err) {
    if (tab) showError(tab, err);
    syncEnv();
  }
}

async function closeFile(path) {
  const tab = activeTab();
  setTabStatus(tab, "Closing file", "busy");
  try {
    const info = await api.closeFile(path);
    if (!renderEnv(info)) return;
    if (!hasFiles()) {
      setTabStatus(tab, "All files closed. Existing query results remain available.");
    } else {
      setTabStatus(tab, `Closed file. Tables: ${tableList()}.`);
    }
  } catch (err) {
    showError(tab, err);
    syncEnv();
  }
}

function openReimport(path) {
  reimportPath = path;
  const file = state.env.files.find((item) => item.source_path === path);
  ui.reimportTarget.textContent = file ? `"${file.file_name}"` : "the file";
  ui.optAbbreviation.value = file?.table ?? "";
  ui.optAbbreviation.setCustomValidity("");
  ui.optDelimiter.value = file?.import_options?.delimiter ?? "";
  const header = file?.import_options?.header;
  ui.optHeader.value = header === null || header === undefined ? "" : String(header);
  ui.dialog.returnValue = "";
  ui.dialog.showModal();
}

async function applyReimport() {
  if (!reimportPath) return;
  const tab = activeTab();
  const path = reimportPath;
  const delimiter = ui.optDelimiter.value || null;
  const headerRaw = ui.optHeader.value;
  const header = headerRaw === "" ? null : headerRaw === "true";
  const abbreviation = ui.optAbbreviation.value.trim();
  setTabStatus(tab, "Updating file", "busy");
  try {
    const info = await api.configureFile(path, { delimiter, header }, abbreviation);
    reimportPath = null;
    if (!renderEnv(info)) return;
    setTabStatus(tab, `Updated. Tables: ${tableList()}.`);
  } catch (err) {
    showError(tab, err);
    syncEnv();
    if (err?.kind === "invalid_abbreviation") {
      reimportPath = path;
      ui.optAbbreviation.setCustomValidity(err.message);
      ui.dialog.returnValue = "";
      ui.dialog.showModal();
      ui.optAbbreviation.reportValidity();
    } else {
      reimportPath = null;
    }
  }
}

async function syncEnv() {
  try {
    renderEnv(await api.envInfo());
  } catch {
    // Best-effort: leave the last known environment visible.
  }
}

// --- query, pagination, and export ---------------------------------------
async function runQuery(tab, sql, page, reuseSession) {
  if (!reuseSession && !hasFiles()) {
    setTabStatus(tab, "Open a file first.");
    return;
  }
  const requestId = beginQueryRequest(tab);
  const previousQueryId = tab.queryId;
  setTabStatus(tab, reuseSession ? "Loading page" : "Running query", "busy");
  try {
    const response = reuseSession && tab.queryId !== null
      ? await api.fetchPage(tab.queryId, page, state.pageSize)
      : await api.startQuery(sql, state.pageSize);
    if (!isCurrentQueryRequest(tab, requestId)) {
      if (!reuseSession) api.cancelQuery(response.query_id).catch(() => {});
      return;
    }

    tab.draftSql = sql;
    tab.sql = sql;
    tab.page = page;
    tab.lastResult = response.result;
    tab.queryId = response.query_id;
    tab.queryRevision = response.workspace_revision;
    if (!reuseSession) tab.totalRows = null;
    setTabStatus(tab, `${response.result.returned.toLocaleString()} rows on this page.`);
    if (tab === activeTab()) renderActiveTab();
    updateWelcome();

    if (!reuseSession && previousQueryId !== null && previousQueryId !== tab.queryId) {
      api.cancelQuery(previousQueryId).catch(() => {});
    }
    if (!reuseSession || tab.totalRows === null) {
      refreshCount(tab, tab.queryId, tab.queryRevision);
    }
  } catch (err) {
    if (isCurrentQueryRequest(tab, requestId)) showError(tab, err);
  }
}

async function refreshCount(tab, queryId, workspaceRevision) {
  try {
    const count = await api.countQuery(queryId);
    if (
      !state.tabs.includes(tab) ||
      tab.queryId !== count.query_id ||
      tab.queryRevision !== workspaceRevision ||
      count.workspace_revision !== workspaceRevision
    ) return;
    tab.totalRows = count.total_rows;
    if (tab === activeTab()) updatePager(tab);
  } catch {
    // Counting is non-fatal; full pages still expose forward pagination.
  }
}

function updatePager(tab) {
  ui.prev.disabled = !canGoPrev(tab);
  ui.next.disabled = !canGoNext(tab);
  if (!tab.lastResult) {
    ui.pageLabel.textContent = "Page 1";
    ui.pageInfo.textContent = "—";
    return;
  }
  const pageHuman = tab.page + 1;
  if (tab.totalRows !== null) {
    const pages = Math.max(1, Math.ceil(tab.totalRows / state.pageSize));
    ui.pageLabel.textContent = `Page ${pageHuman} / ${pages}`;
    ui.pageInfo.textContent = `${tab.totalRows.toLocaleString()} rows total`;
  } else {
    ui.pageLabel.textContent = `Page ${pageHuman}`;
    ui.pageInfo.textContent = "Counting…";
  }
}

async function exportResults() {
  const tab = activeTab();
  if (!tab?.lastResult || tab.queryId === null) return;
  setTabStatus(tab, "Exporting", "busy");
  try {
    const info = await api.exportResults(tab.queryId);
    if (!info) setTabStatus(tab, "Export cancelled.");
    else setTabStatus(tab, `Saved ${info.format} to ${info.path}`);
  } catch (err) {
    showError(tab, err);
  }
}

// --- event wiring --------------------------------------------------------
ui.open.addEventListener("click", addFiles);
ui.welcomeOpen.addEventListener("click", addFiles);
ui.welcomeQuit.addEventListener("click", () => api.quit());
ui.newTab.addEventListener("click", () => addQueryTab());
ui.run.addEventListener("click", () => {
  const tab = activeTab();
  if (tab) runQuery(tab, editor.getValue(), 0, false);
});
ui.reset.addEventListener("click", () => {
  const tab = activeTab();
  if (!tab) return;
  const sql = previewSql();
  tab.autoSql = sql;
  tab.draftSql = sql;
  editor.setValue(sql);
  runQuery(tab, sql, 0, false);
});
ui.prev.addEventListener("click", () => {
  const tab = activeTab();
  if (tab) runQuery(tab, tab.sql, tab.page - 1, true);
});
ui.next.addEventListener("click", () => {
  const tab = activeTab();
  if (tab) runQuery(tab, tab.sql, tab.page + 1, true);
});
ui.export.addEventListener("click", exportResults);

ui.tabs.addEventListener("click", (event) => {
  const close = event.target.closest("[data-close-tab]");
  if (close) {
    event.stopPropagation();
    removeQueryTab(close.dataset.closeTab);
    return;
  }
  const item = event.target.closest("[data-tab-id]");
  if (item) activateTab(item.dataset.tabId);
});

ui.tabs.addEventListener("keydown", (event) => {
  const item = event.target.closest("[data-tab-id]");
  if (!item || event.target.closest("[data-close-tab]")) return;
  const index = state.tabs.findIndex((tab) => tab.id === item.dataset.tabId);
  let nextIndex = null;
  if (event.key === "ArrowRight") nextIndex = (index + 1) % state.tabs.length;
  else if (event.key === "ArrowLeft") nextIndex = (index - 1 + state.tabs.length) % state.tabs.length;
  else if (event.key === "Home") nextIndex = 0;
  else if (event.key === "End") nextIndex = state.tabs.length - 1;
  else if (event.key === "Enter" || event.key === " ") nextIndex = index;
  if (nextIndex === null) return;
  event.preventDefault();
  const id = state.tabs[nextIndex].id;
  activateTab(id);
  ui.tabs.querySelector(`[data-tab-id="${id}"]`)?.focus();
});

ui.files.addEventListener("click", (event) => {
  const button = event.target.closest(".file-chip-btn");
  if (!button) return;
  if (button.classList.contains("close")) closeFile(button.dataset.path);
  else if (button.classList.contains("opts")) openReimport(button.dataset.path);
});

ui.optAbbreviation.addEventListener("input", () => {
  ui.optAbbreviation.setCustomValidity("");
});
ui.dialog.addEventListener("close", () => {
  if (ui.dialog.returnValue === "apply") applyReimport();
  else reimportPath = null;
});

window.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "t") {
    event.preventDefault();
    addQueryTab();
  }
});

// The native File menu owns Cmd/Ctrl+W and targets the focused window. Keeping
// tab cleanup here ensures the keyboard shortcut and close button share the
// same query-session lifecycle.
window.addEventListener("tablebase:close-query-tab", () => {
  if (state.activeTabId) removeQueryTab(state.activeTabId);
});

renderTabs();
renderActiveTab();
syncEnv();
