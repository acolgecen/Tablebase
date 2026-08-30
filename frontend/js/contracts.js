// Runtime-checked command contracts. These checks keep the static, bundler-free
// frontend aligned with Rust DTOs and fail close when either side drifts.

function object(value, name) {
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new TypeError(`Invalid ${name} response.`);
  }
  return value;
}

function number(value, name) {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    throw new TypeError(`Invalid ${name} response.`);
  }
  return value;
}

export function environmentInfo(value) {
  const info = object(value, "environment");
  if (!Array.isArray(info.files)) throw new TypeError("Invalid environment files.");
  number(info.revision, "environment revision");
  for (const file of info.files) {
    object(file, "file");
    number(file.id, "dataset id");
    number(file.row_count, "file row count");
    if (
      typeof file.source_path !== "string" ||
      typeof file.file_name !== "string" ||
      typeof file.table !== "string" ||
      typeof file.cached !== "boolean"
    ) {
      throw new TypeError("Invalid file response.");
    }
    if (!Array.isArray(file.columns)) throw new TypeError("Invalid file columns.");
    for (const column of file.columns) {
      object(column, "column");
      if (typeof column.name !== "string" || typeof column.data_type !== "string") {
        throw new TypeError("Invalid column response.");
      }
    }
    const options = object(file.import_options, "import options");
    if (
      options.delimiter !== null && typeof options.delimiter !== "string" ||
      options.header !== null && typeof options.header !== "boolean"
    ) {
      throw new TypeError("Invalid import options response.");
    }
  }
  return info;
}

export function queryPage(value) {
  const page = object(value, "query page");
  number(page.query_id, "query id");
  number(page.workspace_revision, "query workspace revision");
  const result = object(page.result, "page result");
  if (!Array.isArray(result.columns) || !Array.isArray(result.rows)) {
    throw new TypeError("Invalid page result.");
  }
  number(result.page, "page index");
  number(result.page_size, "page size");
  number(result.returned, "returned row count");
  return page;
}

export function queryCount(value) {
  const count = object(value, "query count");
  number(count.query_id, "query id");
  number(count.workspace_revision, "count workspace revision");
  number(count.total_rows, "total row count");
  return count;
}

export function optionalExportInfo(value) {
  if (value === null) return null;
  const info = object(value, "export");
  if (typeof info.path !== "string" || typeof info.format !== "string") {
    throw new TypeError("Invalid export response.");
  }
  return info;
}
