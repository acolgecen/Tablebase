import assert from "node:assert/strict";
import test from "node:test";
import {
  acceptEnvironment,
  activeTab,
  beginQueryRequest,
  closeQueryTab,
  createQueryTab,
  isCurrentQueryRequest,
  selectQueryTab,
  state,
} from "../js/state.js";

function resetTabs() {
  state.tabs = [];
  state.activeTabId = null;
  state.nextTabId = 1;
}

test("workspace revisions reject stale responses", () => {
  state.env = { files: [], revision: 0 };
  assert.equal(acceptEnvironment({ files: [], revision: 2 }), true);
  assert.equal(acceptEnvironment({ files: [], revision: 1 }), false);
  assert.equal(state.env.revision, 2);
});

test("query generations identify the latest response", () => {
  resetTabs();
  const tab = createQueryTab();
  const first = beginQueryRequest(tab);
  const second = beginQueryRequest(tab);
  assert.equal(isCurrentQueryRequest(tab, first), false);
  assert.equal(isCurrentQueryRequest(tab, second), true);
});

test("query tabs retain independent editor and result state", () => {
  resetTabs();
  const first = createQueryTab("SELECT 1");
  first.lastResult = { rows: [[1]], returned: 1 };
  first.queryId = 41;
  const second = createQueryTab("SELECT 2");
  second.lastResult = { rows: [[2]], returned: 1 };
  second.queryId = 42;

  selectQueryTab(first.id);
  assert.equal(activeTab().draftSql, "SELECT 1");
  assert.equal(activeTab().lastResult.rows[0][0], 1);
  selectQueryTab(second.id);
  assert.equal(activeTab().draftSql, "SELECT 2");
  assert.equal(activeTab().lastResult.rows[0][0], 2);
});

test("closing a tab removes only its retained result", () => {
  resetTabs();
  const first = createQueryTab("SELECT 1");
  first.queryId = 1;
  const second = createQueryTab("SELECT 2");
  second.queryId = 2;
  selectQueryTab(first.id);

  const { removed, active } = closeQueryTab(first.id);
  assert.equal(removed.queryId, 1);
  assert.equal(active.id, second.id);
  assert.equal(state.tabs.length, 1);
  assert.equal(state.tabs[0].queryId, 2);
});
