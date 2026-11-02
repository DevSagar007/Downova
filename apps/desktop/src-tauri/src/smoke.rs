//! Packaged-app smoke test only (`--features smoke`, never in release packages).
//!
//! B0.5 `report`: writes the window and service handshake to `$DOWNOVA_SMOKE_FILE`.
//!
//! B2.1 `bridge`: when `$DOWNOVA_SMOKE_BRIDGE_URL` is set, drives the real UI in the webview —
//! the toolbar's first command (Add URL), the dialog's URL field and its submit — so the link
//! travels the production path (React → `ServiceClient` → Tauri `downloads_add` → service). It
//! then watches the service through the same bridge (`downloads::list`) until that job ends and
//! writes the observed progress to `$DOWNOVA_SMOKE_BRIDGE_FILE`. The smoke script checks the file's
//! SHA-256 itself.
//!
//! B2.5 `watch_only`: `$DOWNOVA_SMOKE_WATCH_URL` / `_FILE` record the progress of a link that
//! arrives from elsewhere (the browser hand-off) through the same `downloads::list` path the UI
//! renders from, without driving the Add URL dialog.

use std::time::{Duration, Instant};

use tauri::Manager;

use crate::{downloads, service};

pub fn report(app: &tauri::AppHandle) {
    let Some(path) = std::env::var_os("DOWNOVA_SMOKE_FILE") else {
        return;
    };
    let window = app.get_webview_window("main").map(|w| {
        (
            w.title().unwrap_or_default(),
            w.is_visible().unwrap_or(false),
        )
    });
    tauri::async_runtime::spawn(async move {
        let service = service::connect().await;
        let report = serde_json::json!({
            "window": window.as_ref().map(|(title, visible)| serde_json::json!({"title": title, "visible": visible})),
            "page_loaded": true,
            "service": service,
            "sidecar": service::sidecar_path().map(|p| p.display().to_string()),
            "pid": std::process::id(),
        });
        let _ = std::fs::write(path, report.to_string());
    });
}

/// Fills and submits the Add URL dialog like a user would (locale-independent selectors).
const DRIVER: &str = r#"(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function until(fn, ms) {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) {
      const v = fn();
      if (v) return v;
      await sleep(100);
    }
    throw new Error("timed out");
  }
  const add = await until(() => {
    const b = document.querySelector('[role="toolbar"] button');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 60000);
  add.click();
  const input = await until(() => document.querySelector('[role="dialog"] input[type="url"]'), 10000);
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  setValue.call(input, __URL__);
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.form.requestSubmit();
})();"#;

pub fn watch_only() {
    let (Ok(url), Some(out)) = (
        std::env::var("DOWNOVA_SMOKE_WATCH_URL"),
        std::env::var_os("DOWNOVA_SMOKE_WATCH_FILE"),
    ) else {
        return;
    };
    tauri::async_runtime::spawn(async move {
        let report = watch(&url, Duration::from_secs(600)).await;
        let _ = std::fs::write(out, report.to_string());
    });
}

pub fn bridge(webview: &tauri::Webview) {
    let (Ok(url), Some(out)) = (
        std::env::var("DOWNOVA_SMOKE_BRIDGE_URL"),
        std::env::var_os("DOWNOVA_SMOKE_BRIDGE_FILE"),
    ) else {
        return;
    };
    let Ok(literal) = serde_json::to_string(&url) else {
        return;
    };
    let _ = webview.eval(DRIVER.replace("__URL__", &literal));
    tauri::async_runtime::spawn(async move {
        let report = watch(&url, Duration::from_secs(180)).await;
        let _ = std::fs::write(out, report.to_string());
    });
}

/// Polls the service until a job for `url` is terminal (or `limit` passes); records each change.
async fn watch(url: &str, limit: Duration) -> serde_json::Value {
    let shown = dm_domain::redact::redact_url(url);
    let started = Instant::now();
    let mut samples: Vec<serde_json::Value> = Vec::new();
    let mut last = None;
    while started.elapsed() < limit {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let Ok(data_dir) = dm_platform::paths::data_dir() else {
            continue;
        };
        let Ok(list) = downloads::list(&data_dir).await else {
            continue;
        };
        // Snapshots show the link redacted (query values removed).
        let Some(job) = list.items.iter().find(|s| s.source_display == shown) else {
            continue;
        };
        let sample = serde_json::json!({
            "ms": started.elapsed().as_millis(),
            "phase": job.phase,
            "desired_state": job.desired_state,
            "downloaded_bytes": job.downloaded_bytes,
            "total_bytes": job.total_bytes,
            "display_name": job.display_name,
        });
        let key = (job.phase, job.downloaded_bytes);
        if last != Some(key) {
            samples.push(sample);
            last = Some(key);
        }
        if job.phase.is_terminal() {
            return serde_json::json!({ "job": job, "jobs_in_list": list.items.len(), "samples": samples });
        }
    }
    serde_json::json!({ "job": null, "samples": samples, "timed_out": true })
}

/// B2.2: appends one lifecycle event (close, second launch, exit) to
/// `$DOWNOVA_SMOKE_LIFECYCLE_FILE` as a JSON line.
pub fn lifecycle_event(event: &str, detail: serde_json::Value) {
    let Some(path) = std::env::var_os("DOWNOVA_SMOKE_LIFECYCLE_FILE") else {
        return;
    };
    let line = serde_json::json!({ "event": event, "detail": detail, "pid": std::process::id() });
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        let _ = writeln!(f, "{line}");
    }
}

pub fn window_state<R: tauri::Runtime>(app: &tauri::AppHandle<R>) -> serde_json::Value {
    let w = app.get_webview_window("main");
    serde_json::json!({
        "visible": w.as_ref().and_then(|w| w.is_visible().ok()),
        "minimized": w.as_ref().and_then(|w| w.is_minimized().ok()),
        "focused": w.as_ref().and_then(|w| w.is_focused().ok()),
    })
}

/// Confirms the "Exit and stop downloads" dialog like a user would (locale-independent).
const CONFIRM_EXIT: &str = r#"(async () => {
  const t0 = Date.now();
  while (Date.now() - t0 < 30000) {
    const b = document.querySelector('[role="dialog"] .btn--primary');
    if (b && !b.hasAttribute("aria-disabled")) { b.click(); return; }
    await new Promise((r) => setTimeout(r, 100));
  }
})();"#;

/// B2.2: when the file `$DOWNOVA_SMOKE_EXIT_WHEN` appears, takes the tray item's path
/// ([`crate::lifecycle::request_exit`]) and confirms the dialog in the real UI.
pub fn exit_driver<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(trigger) = std::env::var_os("DOWNOVA_SMOKE_EXIT_WHEN") else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while !std::path::Path::new(&trigger).exists() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        lifecycle_event("exit_requested", window_state(&app));
        crate::lifecycle::request_exit(&app);
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.eval(CONFIRM_EXIT);
        }
    });
}

/// B2.2: when the file `$DOWNOVA_SMOKE_CLOSE_WHEN` appears, closes the main window through Tauri,
/// which raises the same close request as the window's close button (so the close setting
/// decides). Real title-bar clicks and tray menus are checked manually.
pub fn close_driver<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(trigger) = std::env::var_os("DOWNOVA_SMOKE_CLOSE_WHEN") else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        while !std::path::Path::new(&trigger).exists() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if let Some(w) = app.get_webview_window("main") {
            let _ = w.close();
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        lifecycle_event("after_close", window_state(&app));
    });
}

/// Drives one row command through the real UI: select the row, open the File menu, choose the
/// item (`data-command`), confirm the dialog. Locale-independent selectors.
const ROW_COMMAND: &str = r#"(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function until(fn, ms) {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(100); }
    throw new Error("timed out");
  }
  const row = await until(() => document.querySelector('[data-job-id="__JOB__"]'), 30000);
  row.click();
  const file = await until(() => document.querySelector('[data-menu="1"] .menubar__top'), 5000);
  file.click();
  const item = await until(() => {
    const b = document.querySelector('[data-command="__OP__"]');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 10000);
  item.click();
  const confirm = await until(() => {
    const b = document.querySelector('[role="dialog"] .btn--primary');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 10000);
  confirm.click();
})();"#;

/// B2.7: Clear finished through the same File menu and confirmation, with no job argument.
const CLEAR_COMMAND: &str = r#"(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function until(fn, ms) {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(100); }
    throw new Error("timed out");
  }
  const file = await until(() => document.querySelector('[data-menu="1"] .menubar__top'), 5000);
  file.click();
  const item = await until(() => {
    const b = document.querySelector('[data-command="clearFinished"]');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 10000);
  item.click();
  const confirm = await until(() => {
    const b = document.querySelector('[role="dialog"] .btn--primary');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 10000);
  confirm.click();
})();"#;

/// B2.11: a menu item without a confirmation (`__MENU__` = menu index, `__COMMAND__` = item ID),
/// optionally after selecting a row (`__JOB__`, empty for none).
const MENU_COMMAND: &str = r#"(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function until(fn, ms) {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(100); }
    throw new Error("timed out");
  }
  const job = "__JOB__";
  if (job) {
    const row = await until(() => document.querySelector('[data-job-id="' + job + '"]'), 30000);
    row.click();
  }
  const menu = await until(() => document.querySelector('[data-menu="__MENU__"] .menubar__top'), 5000);
  menu.click();
  const item = await until(() => {
    const b = document.querySelector('[data-command="__COMMAND__"]');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 10000);
  item.click();
})();"#;

/// B2.8: Add URL through the dialog; when the duplicate prompt appears, choose `__CHOICE__`
/// (1 = show the existing download, 2 = download again; 0 = a new link: no prompt is expected,
/// so nothing waits for one — a waiting script could otherwise answer a later prompt).
const ADD_WITH_CHOICE: &str = r#"(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function until(fn, ms) {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(100); }
    throw new Error("timed out");
  }
  const add = await until(() => {
    const b = document.querySelector('[role="toolbar"] button');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 30000);
  add.click();
  const input = await until(() => document.querySelector('[role="dialog"] input[type="url"]'), 10000);
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  setValue.call(input, __URL__);
  input.dispatchEvent(new Event("input", { bubbles: true }));
  const sha = __SHA__;
  if (sha) {
    // B2.18: the dialog's SHA-256 field (the only plain text input of the Add URL form).
    const field = input.form.querySelector('input.add-url__sha');
    setValue.call(field, sha);
    field.dispatchEvent(new Event("input", { bubbles: true }));
  }
  input.form.requestSubmit();
  if (__CHOICE__ === 0) return;
  const choice = await until(() => {
    const b = document.querySelector('.add-url__choices button:nth-child(__CHOICE__)');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 15000);
  choice.click();
})();"#;

/// B2.16: Retry through File → Retry… and its confirmation, whose primary button is pressed twice in
/// a row (a double click): the dialog must send one operation.
const RETRY_COMMAND: &str = r#"(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function until(fn, ms) {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(100); }
    throw new Error("timed out");
  }
  const row = await until(() => document.querySelector('[data-job-id="__JOB__"]'), 30000);
  row.click();
  const file = await until(() => document.querySelector('[data-menu="1"] .menubar__top'), 5000);
  file.click();
  const item = await until(() => {
    const b = document.querySelector('[data-command="redownload"]');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 10000);
  item.click();
  const confirm = await until(() => {
    const b = document.querySelector('[role="dialog"] .btn--primary');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 10000);
  confirm.click();
  confirm.click();
})();"#;

/// B2.17: Refresh link through File → Refresh link…: types `__URL__`, submits, and when the
/// dialog asks (progress cannot be kept) chooses `__CHOICE__` ("keep" = Keep current download,
/// "restart" = Restart from zero; "none" = no question is expected).
const REFRESH_COMMAND: &str = r#"(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  async function until(fn, ms) {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(100); }
    throw new Error("timed out");
  }
  const row = await until(() => document.querySelector('[data-job-id="__JOB__"]'), 30000);
  row.click();
  const file = await until(() => document.querySelector('[data-menu="1"] .menubar__top'), 5000);
  file.click();
  const item = await until(() => {
    const b = document.querySelector('[data-command="refreshLink"]');
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 10000);
  item.click();
  const input = await until(() => document.querySelector('[role="dialog"] input[type="url"]'), 10000);
  const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
  setValue.call(input, __URL__);
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.form.requestSubmit();
  const choice = "__CHOICE__";
  if (choice === "none") return;
  // The question replaces the footer: secondary = Keep current download, primary = Restart.
  await until(() => document.querySelector('[role="dialog"] [role="alert"] .banner'), 30000);
  const pick = await until(() => {
    const b = document.querySelector(
      choice === "restart" ? '[role="dialog"] .btn--primary' : '[role="dialog"] .dialog__footer .btn--secondary',
    );
    return b && !b.hasAttribute("aria-disabled") ? b : null;
  }, 10000);
  pick.click();
})();"#;

/// ADV12: the virtualized list with the keyboard, as a user would — focus the first row, End,
/// then Home — and its state after each step. The page cannot write files, so the result goes into
/// the page URL's fragment (`history.replaceState`, no navigation); the Rust side reads it back with
/// `url()` and records it. The native window title does not follow `document.title` (smoke only).
const LIST_PROBE: &str = r##"(async () => {
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const frame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  async function until(fn, ms) {
    const t0 = Date.now();
    while (Date.now() - t0 < ms) { const v = fn(); if (v) return v; await sleep(50); }
    return null;
  }
  const out = { ok: false };
  try {
    const first = await until(() => document.querySelector("[data-job-id]"), 60000);
    const table = document.querySelector("table.download-table");
    out.rowcount = Number(table.getAttribute("aria-rowcount"));
    out.rendered = document.querySelectorAll("[data-job-id]").length;
    first.focus();
    const key = (k) => document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true }));
    const t0 = performance.now();
    key("End");
    const lastIndex = String(out.rowcount);
    const last = await until(() => document.activeElement?.closest("tr")?.getAttribute("aria-rowindex") === lastIndex ? document.activeElement : null, 10000);
    await frame();
    out.end_ms = performance.now() - t0;
    out.end_focused_index = Number(document.activeElement?.closest("tr")?.getAttribute("aria-rowindex"));
    out.end_selected = last?.getAttribute("aria-pressed") === "true";
    out.rendered_after_end = document.querySelectorAll("[data-job-id]").length;
    key("Home");
    await until(() => document.activeElement?.closest("tr")?.getAttribute("aria-rowindex") === "2", 10000);
    out.home_focused_index = Number(document.activeElement?.closest("tr")?.getAttribute("aria-rowindex"));
    out.spacers_hidden = [...document.querySelectorAll("tr.download-table__spacer")].every((s) => s.getAttribute("aria-hidden") === "true");
    // B2.24, in the real window: reflow at page zoom 100/150/200 % (200 % also crosses the narrow
    // layout breakpoints) in both themes, and a Bangla search over the whole dataset.
    const root = document.documentElement;
    const overflow = () => Math.max(0, root.scrollWidth - root.clientWidth);
    out.layout = [];
    const themeBefore = root.dataset.theme;
    for (const theme of ["light", "dark"]) {
      root.dataset.theme = theme;
      for (const z of [1, 1.5, 2]) {
        root.style.zoom = String(z);
        await frame();
        await sleep(150);
        const focused = document.activeElement;
        // Rendered rows must cover the list viewport (no blank band), also scrolled to the middle.
        const sc = document.querySelector(".download-table__scroll");
        const gap = async (top) => {
          sc.scrollTop = top;
          sc.dispatchEvent(new Event("scroll"));
          await frame();
          await sleep(150);
          const box = sc.getBoundingClientRect();
          const head = sc.querySelector("thead").getBoundingClientRect().height;
          const rows = [...sc.querySelectorAll("tbody tr[data-row]")].map((r) => r.getBoundingClientRect()).filter((r) => r.bottom > box.top + head && r.top < box.bottom);
          if (rows.length === 0) return Math.round(box.height);
          const shownTop = Math.min(...rows.map((r) => r.top));
          const shownBottom = Math.max(...rows.map((r) => r.bottom));
          const above = Math.max(0, shownTop - (box.top + head));
          const below = Math.max(0, box.bottom - shownBottom);
          const blank = Math.round(above + below);
          // Where a band appears, record why (B2.30: Linux showed 6 px at the top at 200 %): which
          // side, the scroller and header boxes, the rendered window and the scroller units.
          // Rounded to 0.1 px to keep the smoke annotation short.
          if (blank > 1) {
            const r1 = (v) => Math.round(v * 10) / 10;
            const indexes = [...sc.querySelectorAll("tbody tr[data-row]")].map((r) => Number(r.getAttribute("aria-rowindex")));
            const spacers = [...sc.querySelectorAll("tr.download-table__spacer td")].map((td) => r1(td.getBoundingClientRect().height));
            geometry.push({
              at: top === 0 ? "top" : "middle", above: r1(above), below: r1(below),
              box: [r1(box.top), r1(box.bottom)], head: r1(head), offsetHeight: sc.offsetHeight, clientHeight: sc.clientHeight,
              scrollTop: r1(sc.scrollTop), shown: [r1(shownTop), r1(shownBottom)], first_row_h: r1(rows[0].height),
              indexes: [Math.min(...indexes), Math.max(...indexes)], spacers,
            });
          }
          return blank;
        };
        const geometry = [];
        const keep = sc.scrollTop;
        const blankMiddle = await gap(sc.scrollHeight / 2);
        const blankTop = await gap(0);
        sc.scrollTop = keep;
        sc.dispatchEvent(new Event("scroll"));
        await frame();
        out.layout.push({
          blank_px_middle: blankMiddle,
          blank_px_top: blankTop,
          ...(geometry.length ? { geometry } : {}),
          theme,
          zoom: z,
          page_overflow_px: overflow(),
          rendered: document.querySelectorAll("[data-job-id]").length,
          focus_kept: !!focused && focused.matches("[data-job-id]"),
          background: getComputedStyle(document.body).backgroundColor,
        });
      }
    }
    root.style.zoom = "";
    if (themeBefore === undefined) delete root.dataset.theme; else root.dataset.theme = themeBefore;
    const input = document.querySelector("#download-search");
    const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set;
    const search = async (q) => {
      setValue.call(input, q);
      input.dispatchEvent(new Event("input", { bubbles: true }));
      await frame();
      await sleep(100);
      return Number(table.getAttribute("aria-rowcount"));
    };
    out.bangla_search_rowcount = await search("প্রতিবেদন");
    out.bangla_name_shown = [...document.querySelectorAll("[data-job-id]")].some((b) => (b.textContent ?? "").includes("দলিল-প্রতিবেদন"));
    out.cleared_rowcount = await search("");
    out.ok = true;
  } catch (e) {
    out.error = String(e);
  }
  history.replaceState(null, "", "#probe:" + encodeURIComponent(JSON.stringify(out)));
})();"##;

/// B2.3/B2.7: each line `{"op": "cancel" | "remove", "job": "job_…"}` or
/// `{"op": "clear"}` appended to `$DOWNOVA_SMOKE_COMMANDS_FILE` is performed once through the real
/// menus and dialogs. The smoke script checks the outcome through the service and the file system.
pub fn command_driver<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    let Some(path) = std::env::var_os("DOWNOVA_SMOKE_COMMANDS_FILE") else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut done = 0usize;
        loop {
            tokio::time::sleep(Duration::from_millis(200)).await;
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // Only complete lines: the script may be writing the next one.
            let complete = text.rsplit_once('\n').map_or("", |(lines, _)| lines);
            for line in complete.lines().skip(done) {
                done = done.saturating_add(1);
                let Ok(cmd) = serde_json::from_str::<serde_json::Value>(line) else {
                    continue;
                };
                let Some(op) = cmd["op"].as_str() else {
                    continue;
                };
                // B2.8–B2.11 ops: fixed menus, commands and choices only.
                if op == "menu" {
                    let command = cmd["command"].as_str().unwrap_or_default();
                    let job = cmd["job"].as_str().unwrap_or_default();
                    let (menu, allowed) = match command {
                        "startQueue" | "stopQueue" => ("2", job.is_empty()),
                        "moveTop" | "moveBottom" | "moveUp" | "moveDown" => (
                            "1",
                            job.starts_with("job_")
                                && job.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
                        ),
                        _ => ("", false),
                    };
                    if allowed && let Some(w) = app.get_webview_window("main") {
                        let _ = w.eval(
                            MENU_COMMAND
                                .replace("__JOB__", job)
                                .replace("__MENU__", menu)
                                .replace("__COMMAND__", command),
                        );
                    }
                    lifecycle_event(
                        "row_command",
                        serde_json::json!({ "op": op, "command": command }),
                    );
                    continue;
                }
                if op == "add" {
                    let choice = match cmd["choice"].as_str() {
                        Some("none") => "0",
                        Some("show") => "1",
                        Some("again") => "2",
                        _ => continue,
                    };
                    let (Some(url), Some(w)) =
                        (cmd["url"].as_str(), app.get_webview_window("main"))
                    else {
                        continue;
                    };
                    let Ok(literal) = serde_json::to_string(url) else {
                        continue;
                    };
                    // B2.18: an optional SHA-256, accepted only as 64 hex digits.
                    let sha = cmd["sha256"]
                        .as_str()
                        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
                        .map_or_else(|| "null".to_owned(), |s| format!("\"{s}\""));
                    let _ = w.eval(
                        ADD_WITH_CHOICE
                            .replace("__URL__", &literal)
                            .replace("__SHA__", &sha)
                            .replace("__CHOICE__", choice),
                    );
                    lifecycle_event(
                        "row_command",
                        serde_json::json!({ "op": op, "choice": choice }),
                    );
                    continue;
                }
                if op == "list_probe" {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.eval(LIST_PROBE);
                        let started = Instant::now();
                        let mut result = serde_json::Value::Null;
                        while started.elapsed() < Duration::from_secs(90) {
                            tokio::time::sleep(Duration::from_millis(200)).await;
                            let fragment = w
                                .url()
                                .ok()
                                .and_then(|u| u.fragment().map(str::to_owned))
                                .and_then(|f| f.strip_prefix("probe:").map(str::to_owned));
                            if let Some(encoded) = fragment {
                                let json = percent_decode(&encoded);
                                result = serde_json::from_str(&json).unwrap_or_default();
                                break;
                            }
                        }
                        let _ = w.eval("history.replaceState(null, \"\", location.pathname)");
                        lifecycle_event("list_probe", result);
                    }
                    continue;
                }
                // B2.16/B2.17: Retry and Refresh link through the real menus and dialogs.
                if op == "retry" || op == "refresh" {
                    let job = cmd["job"].as_str().unwrap_or_default();
                    let valid_job = job.starts_with("job_")
                        && job.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
                    let choice = cmd["choice"].as_str().unwrap_or("none");
                    if !valid_job || !matches!(choice, "none" | "keep" | "restart") {
                        continue;
                    }
                    let Some(w) = app.get_webview_window("main") else {
                        continue;
                    };
                    if op == "retry" {
                        let _ = w.eval(RETRY_COMMAND.replace("__JOB__", job));
                    } else {
                        let Some(literal) = cmd["url"]
                            .as_str()
                            .and_then(|u| serde_json::to_string(u).ok())
                        else {
                            continue;
                        };
                        let _ = w.eval(
                            REFRESH_COMMAND
                                .replace("__JOB__", job)
                                .replace("__URL__", &literal)
                                .replace("__CHOICE__", choice),
                        );
                    }
                    // The link is never recorded.
                    lifecycle_event(
                        "row_command",
                        serde_json::json!({ "op": op, "job": job, "choice": choice }),
                    );
                    continue;
                }
                if op == "clear" {
                    if let Some(w) = app.get_webview_window("main") {
                        let _ = w.eval(CLEAR_COMMAND);
                    }
                    lifecycle_event("row_command", serde_json::json!({ "op": op }));
                    continue;
                }
                let Some(job) = cmd["job"].as_str() else {
                    continue;
                };
                // Only the row commands and a job ID shape reach the page.
                let valid_job = job.starts_with("job_")
                    && job.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
                if !matches!(op, "cancel" | "remove") || !valid_job {
                    continue;
                }
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.eval(ROW_COMMAND.replace("__JOB__", job).replace("__OP__", op));
                }
                lifecycle_event("row_command", serde_json::json!({ "op": op, "job": job }));
            }
        }
    });
}

/// Decodes `encodeURIComponent` output (smoke probe results only).
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b'%') => {
                let hex = s
                    .get(i + 1..i + 3)
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                if let Some(v) = hex {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            Some(b) => out.push(*b),
            None => {}
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}
