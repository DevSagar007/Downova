// DevTools over a WebView2 page's remote-debugging port (the packaged window's real UI). Shared by
// the packaged GUI acceptance scripts. `ui(expr)` evaluates `expr` (an expression, usually an async
// IIFE) in the page with small helpers in scope:
//   sleep(ms), frame(), until(fn, ms), labelled(text), setValue(el, v), byText(sel, re), click(el)
// `ipc` lists the page's Tauri IPC requests ("METHOD url") once `Network.enable` was sent.
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function devtools(port) {
  let target;
  for (let i = 0; i < 150 && !target; i++) {
    try {
      const list = await (
        await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(5000) })
      ).json();
      target = list.find((t) => t.type === "page");
    } catch {
      /* not up yet */
    }
    if (!target) await sleep(200);
  }
  if (!target) throw new Error("no WebView DevTools page");
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  const ipc = [];
  await new Promise((r, j) => {
    ws.onopen = r;
    ws.onerror = j;
  });
  let id = 0;
  const waiting = new Map();
  ws.onmessage = (m) => {
    const msg = JSON.parse(m.data);
    if (
      msg.method === "Network.requestWillBeSent" &&
      /ipc\.localhost|^ipc:/.test(msg.params.request.url)
    )
      ipc.push(`${msg.params.request.method} ${msg.params.request.url}`);
    waiting.get(msg.id)?.(msg);
    waiting.delete(msg.id);
  };
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      const timer = setTimeout(() => reject(new Error(`${method} timed out`)), 60_000);
      waiting.set(n, (msg) => {
        clearTimeout(timer);
        if (msg.error) reject(new Error(msg.error.message));
        else resolve(msg.result);
      });
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  const HELPERS = `
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    const frame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const until = async (fn, ms) => { const t0 = performance.now(); while (performance.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(50); } throw new Error("page wait timed out: " + fn.toString().slice(0, 160)); };
    const labelled = (text, root = document) => { const l = [...root.querySelectorAll("label")].find((x) => x.textContent.trim() === text); if (!l) return null; return l.htmlFor ? document.getElementById(l.htmlFor) : l.querySelector("input,select,textarea"); };
    const setValue = (el, v) => { const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype; Object.getOwnPropertyDescriptor(proto, "value").set.call(el, v); el.dispatchEvent(new Event("input", { bubbles: true })); el.dispatchEvent(new Event("change", { bubbles: true })); };
    const byText = (sel, re, root = document) => [...root.querySelectorAll(sel)].find((e) => re.test(e.textContent.trim()));
    const click = (el) => { if (!el) throw new Error("nothing to click"); el.click(); return true; };`;
  const ui = async (expr) => {
    // The page may still be loading when DevTools attaches: retry a destroyed context.
    for (let k = 0; k < 5; k++) {
      const r = await send("Runtime.evaluate", {
        expression: `(async () => { ${HELPERS} return await (${expr}); })()`,
        awaitPromise: true,
        returnByValue: true,
      });
      if (r.exceptionDetails) {
        const text = r.exceptionDetails.exception?.description ?? r.exceptionDetails.text;
        if (/context was destroyed|Cannot find context/i.test(text)) {
          await sleep(1000);
          continue;
        }
        throw new Error(text);
      }
      return r.result?.value;
    }
    throw new Error("the page kept reloading");
  };
  const screenshot = async () => {
    const png = await send("Page.captureScreenshot", { format: "png" });
    return Buffer.from(png.data, "base64");
  };
  return { send, ui, ipc, screenshot, close: () => ws.close() };
}
