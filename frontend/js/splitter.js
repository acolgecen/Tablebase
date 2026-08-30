// Draggable horizontal splitter. Dragging the handle updates a CSS variable on
// :root that drives the editor pane's grid row height. Node-free.

/**
 * @param {HTMLElement} handle  The grab bar.
 * @param {object} opts
 *   cssVar  - CSS custom property to update (default "--editor-h")
 *   min     - minimum editor height in px
 *   reserve - px to keep available below the editor (for results)
 */
export function initSplitter(handle, { cssVar = "--editor-h", min = 90, reserve = 220 } = {}) {
  const root = document.documentElement;
  let startY = 0;
  let startH = 0;
  let dragging = false;

  function currentHeight() {
    const v = getComputedStyle(root).getPropertyValue(cssVar).trim();
    return parseInt(v, 10) || 170;
  }

  function onMove(e) {
    if (!dragging) return;
    const max = Math.max(min, window.innerHeight - reserve);
    const next = Math.max(min, Math.min(max, startH + (e.clientY - startY)));
    root.style.setProperty(cssVar, `${next}px`);
  }

  function onUp() {
    dragging = false;
    document.body.classList.remove("resizing");
    window.removeEventListener("mousemove", onMove);
    window.removeEventListener("mouseup", onUp);
  }

  handle.addEventListener("mousedown", (e) => {
    dragging = true;
    startY = e.clientY;
    startH = currentHeight();
    document.body.classList.add("resizing");
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
    e.preventDefault();
  });
}
