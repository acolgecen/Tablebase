import assert from "node:assert/strict";
import test from "node:test";
import {
  acceptEnvironment,
  beginQueryRequest,
  isCurrentQueryRequest,
  state,
} from "../js/state.js";

test("workspace revisions reject stale responses", () => {
  state.env = { files: [], revision: 0 };
  assert.equal(acceptEnvironment({ files: [], revision: 2 }), true);
  assert.equal(acceptEnvironment({ files: [], revision: 1 }), false);
  assert.equal(state.env.revision, 2);
});

test("query generations identify the latest response", () => {
  const first = beginQueryRequest();
  const second = beginQueryRequest();
  assert.equal(isCurrentQueryRequest(first), false);
  assert.equal(isCurrentQueryRequest(second), true);
});
