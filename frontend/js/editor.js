// SQL editor with Monokai syntax highlighting — Node-free, no dependencies.
//
// Implementation: a SINGLE `contenteditable` element. The caret lives directly
// in the highlighted text, so the cursor is always exactly where the next
// character will land (no transparent-overlay misalignment). On each edit we
// re-tokenize, replace the markup, and restore the caret by character offset —
// the lightweight technique popularized by CodeJar.
//
// `contenteditable="plaintext-only"` (WebKit-native) makes Enter and paste
// insert plain text, so the content stays clean `\n`-separated text with no
// stray <div>/<br> nodes.

// Word sets are upper-cased; lookups upper-case the token first.
const KEYWORDS = new Set(
  `SELECT FROM WHERE GROUP BY HAVING ORDER LIMIT OFFSET AS JOIN INNER LEFT RIGHT
   FULL OUTER CROSS NATURAL ON USING UNION ALL EXCEPT INTERSECT WITH RECURSIVE
   DISTINCT AND OR NOT IN IS LIKE ILIKE SIMILAR BETWEEN CASE WHEN THEN ELSE END
   ASC DESC NULLS FIRST LAST OVER PARTITION WINDOW ROWS RANGE GROUPS UNBOUNDED
   PRECEDING FOLLOWING CURRENT ROW EXISTS ANY SOME CAST TRY_CAST FILTER QUALIFY
   VALUES TABLE PIVOT UNPIVOT SAMPLE LATERAL`.split(/\s+/),
);
const FUNCTIONS = new Set(
  `COUNT SUM AVG MIN MAX ROUND ABS CEIL FLOOR COALESCE NULLIF LENGTH LOWER UPPER
   TRIM LTRIM RTRIM SUBSTRING SUBSTR REPLACE CONCAT CONCAT_WS SPLIT_PART STRFTIME
   STRPTIME DATE_TRUNC DATE_PART DATE_DIFF DATEDIFF EXTRACT NOW CURRENT_DATE
   CURRENT_TIMESTAMP ROW_NUMBER RANK DENSE_RANK LAG LEAD FIRST_VALUE LAST_VALUE
   NTILE STDDEV VARIANCE MEDIAN MODE QUANTILE LIST ARRAY_AGG STRING_AGG
   REGEXP_MATCHES REGEXP_REPLACE REGEXP_EXTRACT GREATEST LEAST IFNULL`.split(/\s+/),
);
const TYPES = new Set(
  `INT INTEGER BIGINT SMALLINT TINYINT HUGEINT UINTEGER UBIGINT DOUBLE FLOAT REAL
   DECIMAL NUMERIC VARCHAR TEXT CHAR STRING BOOLEAN BOOL DATE TIME TIMESTAMP
   TIMESTAMPTZ INTERVAL BLOB BYTEA UUID JSON STRUCT MAP`.split(/\s+/),
);
const CONSTANTS = new Set(["TRUE", "FALSE", "NULL"]);

// One pass: comments | single-quoted string | double-quoted ident | number |
// word | operator. Anything unmatched falls through as plain text.
const TOKEN =
  /(--[^\n]*|\/\*[\s\S]*?\*\/)|('(?:[^']|'')*')|("(?:[^"]|"")*")|(\b\d+(?:\.\d+)?\b)|([A-Za-z_][A-Za-z0-9_]*)|([-+/%<>=!|&^~]+)/g;

function escapeHtml(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function span(cls, text) {
  return `<span class="${cls}">${escapeHtml(text)}</span>`;
}

function highlight(code) {
  let out = "";
  let last = 0;
  let m;
  TOKEN.lastIndex = 0;
  while ((m = TOKEN.exec(code))) {
    if (m.index > last) out += escapeHtml(code.slice(last, m.index));
    last = TOKEN.lastIndex;
    const t = m[0];
    if (m[1]) out += span("cm-comment", t);
    else if (m[2]) out += span("cm-string", t);
    else if (m[3]) out += span("cm-ident-quoted", t);
    else if (m[4]) out += span("cm-number", t);
    else if (m[5]) {
      const up = t.toUpperCase();
      if (KEYWORDS.has(up)) out += span("cm-keyword", t);
      else if (CONSTANTS.has(up)) out += span("cm-constant", t);
      else if (TYPES.has(up)) out += span("cm-type", t);
      else if (FUNCTIONS.has(up)) out += span("cm-function", t);
      else out += escapeHtml(t);
    } else if (m[6]) out += span("cm-operator", t);
    else out += escapeHtml(t);
  }
  if (last < code.length) out += escapeHtml(code.slice(last));
  return out;
}

// --- caret save/restore by absolute character offset ----------------------

function getCaretOffset(el) {
  const sel = window.getSelection();
  if (!sel || sel.rangeCount === 0) return null;
  const range = sel.getRangeAt(0);
  if (!el.contains(range.startContainer)) return null;
  const measure = range.cloneRange();
  measure.selectNodeContents(el);
  measure.setEnd(range.startContainer, range.startOffset);
  return measure.toString().length;
}

function setCaretOffset(el, offset) {
  if (offset == null) return;
  const sel = window.getSelection();
  const range = document.createRange();
  const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, null);
  let remaining = offset;
  let node;
  let placed = false;
  while ((node = walker.nextNode())) {
    const len = node.nodeValue.length;
    if (remaining <= len) {
      range.setStart(node, remaining);
      placed = true;
      break;
    }
    remaining -= len;
  }
  if (!placed) {
    range.selectNodeContents(el);
    range.collapse(false);
  } else {
    range.collapse(true);
  }
  sel.removeAllRanges();
  sel.addRange(range);
}

/**
 * Wire up the editor inside `root` (which must contain a `.sql-input`
 * contenteditable element). `onRun` fires on Cmd/Ctrl+Enter.
 * Returns { getValue, setValue, focus }.
 */
export function createEditor(root, { onRun } = {}) {
  const ed = root.querySelector(".sql-input");
  let composing = false; // don't repaint mid-IME-composition

  function paint() {
    const code = ed.textContent;
    const caret = getCaretOffset(ed);
    ed.innerHTML = highlight(code);
    setCaretOffset(ed, caret);
  }

  ed.addEventListener("input", () => {
    if (composing) return;
    paint();
  });
  ed.addEventListener("compositionstart", () => {
    composing = true;
  });
  ed.addEventListener("compositionend", () => {
    composing = false;
    paint();
  });

  ed.addEventListener("keydown", (e) => {
    if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      onRun?.();
    } else if (e.key === "Tab") {
      e.preventDefault();
      document.execCommand("insertText", false, "  ");
    }
  });

  return {
    getValue: () => ed.textContent,
    setValue: (v) => {
      ed.textContent = v;
      paint();
    },
    focus: () => ed.focus(),
  };
}
