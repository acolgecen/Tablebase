// Single source of truth for this window's UI. Plain mutable object — small
// enough that a framework would be overkill, explicit enough for a code agent
// to follow. Each window runs its own copy of the frontend and its own `state`.

export const state = {
  /** EnvironmentInfo for this window: { files: FileInfo[] }. */
  env: { files: [], revision: 0 },
  /** The SQL currently driving the results grid. */
  sql: "",
  /** The preview SQL we last auto-inserted (so we only replace *our* text). */
  autoSql: "",
  /** Zero-based page index of the grid. */
  page: 0,
  /** Rows per page (the spec's default 2000). */
  pageSize: 2000,
  /** Total rows of the current query once counted, else null (unknown). */
  totalRows: null,
  /** The most recent PageResult, or null. */
  lastResult: null,
  /** Immutable backend query session currently represented by the grid. */
  queryId: null,
  queryRevision: null,
  /** Monotonic client generation used to reject out-of-order responses. */
  requestSequence: 0,
  activeQueryRequest: 0,
};

export function beginQueryRequest() {
  const id = ++state.requestSequence;
  state.activeQueryRequest = id;
  return id;
}

export function isCurrentQueryRequest(id) {
  return state.activeQueryRequest === id;
}

/** Apply only monotonic workspace snapshots; concurrent command responses can
 * arrive out of order even though backend mutations are serialized. */
export function acceptEnvironment(info) {
  if (info.revision < state.env.revision) return false;
  state.env = info;
  return true;
}

/** Whether this window has any file open. */
export function hasFiles() {
  return state.env.files.length > 0;
}

/** The first file's current automatic or user-selected SQL abbreviation. */
export function firstTable() {
  return state.env.files[0]?.table ?? "data";
}

/** Comma-separated list of the current table names, for status messages. */
export function tableList() {
  return state.env.files.map((f) => f.table).join(", ");
}

/** The default query shown after opening files. */
export function previewSql() {
  return `SELECT * FROM ${firstTable()}`;
}

/** True if the grid currently has displayable results. */
export function hasResults() {
  return state.lastResult !== null;
}

/** Whether a "next page" is possible given what we know. */
export function canGoNext() {
  if (!state.lastResult) return false;
  if (state.totalRows !== null) {
    return (state.page + 1) * state.pageSize < state.totalRows;
  }
  // Count not known yet: allow next only if this page was full.
  return state.lastResult.returned >= state.pageSize;
}

export function canGoPrev() {
  return state.page > 0;
}
