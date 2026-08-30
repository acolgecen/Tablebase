// Renders a PageResult into an HTML table. Values arrive pre-rendered as text
// (or null) from the backend, so this layer only does DOM, never formatting.

/**
 * Render a page into the grid container.
 * @param {HTMLElement} container
 * @param {object} result  PageResult { columns, rows, ... }
 * @param {number} startIndex  Zero-based index of the first row (for numbering).
 */
export function renderGrid(container, result, startIndex) {
  const { columns, rows } = result;

  if (rows.length === 0) {
    container.innerHTML = `<div class="empty-state muted">No rows.</div>`;
    return;
  }

  const table = document.createElement("table");
  table.className = "data-table";

  // Header: a leading row-number column, then each query column with its type.
  const thead = document.createElement("thead");
  const headRow = document.createElement("tr");
  headRow.appendChild(th("#", "row-num"));
  for (const col of columns) {
    const cell = document.createElement("th");
    cell.appendChild(document.createTextNode(col.name));
    const type = document.createElement("span");
    type.className = "col-type";
    type.textContent = col.data_type;
    cell.appendChild(type);
    headRow.appendChild(cell);
  }
  thead.appendChild(headRow);
  table.appendChild(thead);

  // Body.
  const tbody = document.createElement("tbody");
  rows.forEach((row, r) => {
    const tr = document.createElement("tr");
    tr.appendChild(td(String(startIndex + r + 1), "row-num"));
    for (const value of row) {
      if (value === null) {
        tr.appendChild(td("NULL", "cell-null"));
      } else {
        const cell = td(value);
        cell.title = value; // full value on hover (cells are clipped)
        tr.appendChild(cell);
      }
    }
    tbody.appendChild(tr);
  });
  table.appendChild(tbody);

  container.replaceChildren(table);
}

/** Show an empty-state message. */
export function renderEmpty(container, message) {
  container.innerHTML = `<div class="empty-state muted">${message}</div>`;
}

function th(text, className) {
  const el = document.createElement("th");
  el.textContent = text;
  if (className) el.className = className;
  return el;
}

function td(text, className) {
  const el = document.createElement("td");
  el.textContent = text;
  if (className) el.className = className;
  return el;
}
