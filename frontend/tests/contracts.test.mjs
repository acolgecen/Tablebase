import assert from "node:assert/strict";
import test from "node:test";
import {
  environmentInfo,
  optionalExportInfo,
  queryCount,
  queryPage,
} from "../js/contracts.js";

test("accepts backend DTO shapes", () => {
  const environment = {
    revision: 1,
    files: [{
      id: 0,
      source_path: "/tmp/data.csv",
      file_name: "data.csv",
      table: "data",
      columns: [],
      row_count: 0,
      cached: false,
      import_options: { delimiter: null, header: null },
    }],
  };
  assert.equal(environmentInfo(environment), environment);
  assert.equal(queryPage({
    query_id: 2,
    workspace_revision: 1,
    result: { columns: [], rows: [], page: 0, page_size: 2000, returned: 0 },
  }).query_id, 2);
  assert.equal(queryCount({
    query_id: 2,
    workspace_revision: 1,
    total_rows: 0,
  }).total_rows, 0);
  assert.equal(optionalExportInfo(null), null);
});

test("rejects contract drift", () => {
  assert.throws(() => environmentInfo({ files: [] }), TypeError);
  assert.throws(() => queryPage({ query_id: "2" }), TypeError);
});
