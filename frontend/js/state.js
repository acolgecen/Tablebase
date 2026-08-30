// Window-scoped UI state. The file environment is shared by every query tab,
// while editor text, results, pagination, and backend query sessions belong to
// one tab and live until that tab (or its window) is closed.

export const state = {
  env: { files: [], revision: 0 },
  tabs: [],
  activeTabId: null,
  nextTabId: 1,
  pageSize: 2000,
};

export function createQueryTab(sql = "") {
  const number = state.nextTabId++;
  const tab = {
    id: `query-${number}`,
    title: `Query ${number}`,
    draftSql: sql,
    sql: "",
    autoSql: "",
    page: 0,
    totalRows: null,
    lastResult: null,
    queryId: null,
    queryRevision: null,
    requestSequence: 0,
    activeQueryRequest: 0,
    busy: false,
    status: "Ready.",
    statusKind: "",
  };
  state.tabs.push(tab);
  state.activeTabId = tab.id;
  return tab;
}

export function activeTab() {
  return state.tabs.find((tab) => tab.id === state.activeTabId) ?? null;
}

export function queryTab(id) {
  return state.tabs.find((tab) => tab.id === id) ?? null;
}

export function selectQueryTab(id) {
  if (!queryTab(id)) return null;
  state.activeTabId = id;
  return activeTab();
}

/** Remove a tab and return both the removed tab and the newly active one. */
export function closeQueryTab(id) {
  const index = state.tabs.findIndex((tab) => tab.id === id);
  if (index < 0) return { removed: null, active: activeTab() };
  const [removed] = state.tabs.splice(index, 1);
  if (state.activeTabId === id) {
    state.activeTabId = state.tabs[Math.min(index, state.tabs.length - 1)]?.id ?? null;
  }
  return { removed, active: activeTab() };
}

export function beginQueryRequest(tab) {
  const id = ++tab.requestSequence;
  tab.activeQueryRequest = id;
  return id;
}

export function isCurrentQueryRequest(tab, id) {
  return state.tabs.includes(tab) && tab.activeQueryRequest === id;
}

/** Apply only monotonic workspace snapshots; concurrent command responses can
 * arrive out of order even though backend mutations are serialized. */
export function acceptEnvironment(info) {
  if (info.revision < state.env.revision) return false;
  state.env = info;
  return true;
}

export function hasFiles() {
  return state.env.files.length > 0;
}

export function hasPersistedResults() {
  return state.tabs.some((tab) => tab.lastResult !== null);
}

export function firstTable() {
  return state.env.files[0]?.table ?? "data";
}

export function tableList() {
  return state.env.files.map((file) => file.table).join(", ");
}

export function previewSql() {
  return `SELECT * FROM ${firstTable()}`;
}

export function canGoNext(tab = activeTab()) {
  if (!tab?.lastResult) return false;
  if (tab.totalRows !== null) {
    return (tab.page + 1) * state.pageSize < tab.totalRows;
  }
  return tab.lastResult.returned >= state.pageSize;
}

export function canGoPrev(tab = activeTab()) {
  return (tab?.page ?? 0) > 0;
}

// Every window begins with one scratch query.
createQueryTab();
