import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

// Evidence for a browser that starts but never answers DevTools (B2.8 baseline repair). Collected
// before the browser is killed, so it shows what was actually running: the launcher (a shell
// wrapper or the binary), the process tree it produced, whether the DevTools pipe (fds 3 and 4)
// reached the real browser process, and what the profile directory held.

/** Rows of `ps -eo pid=,ppid=,stat=,etimes=,args=` → objects. */
export function parsePs(text) {
  const rows = [];
  for (const line of text.split("\n")) {
    const m = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(\d+)\s+(.*)$/.exec(line);
    if (m)
      rows.push({
        pid: Number(m[1]),
        ppid: Number(m[2]),
        stat: m[3],
        elapsed_s: Number(m[4]),
        args: m[5],
      });
  }
  return rows;
}

/** `root` and every process below it, parents first. */
export function descendants(rows, root) {
  const out = [];
  const seen = new Set();
  let frontier = [root];
  while (frontier.length) {
    const next = [];
    for (const pid of frontier) {
      if (seen.has(pid)) continue;
      seen.add(pid);
      const row = rows.find((r) => r.pid === pid);
      if (row) out.push(row);
      for (const child of rows) if (child.ppid === pid) next.push(child.pid);
    }
    frontier = next;
  }
  return out;
}

/** What a launcher file is: a script (with the lines that redirect or exec) or a binary. */
export function launcherInfo(head) {
  if (!head.startsWith("#!")) return { kind: "binary" };
  const lines = head.split("\n");
  return {
    kind: "script",
    shebang: lines[0],
    // The lines that decide which process gets the DevTools pipe and the standard streams.
    exec_lines: lines
      .map((l) => l.trim())
      .filter((l) => /^exec\b|^"\$HERE|\bexec -a\b/.test(l))
      .slice(0, 12),
  };
}

/** Command lines can carry the profile path but never secrets; keep them bounded anyway. */
const short = (s, n = 300) => (s.length > n ? `${s.slice(0, n)}…` : s);

export function collectBrowserDiagnostics({ platform, pid, browserPath, profile }) {
  const diag = { platform, launcher_path: browserPath };
  try {
    diag.launcher_realpath = fs.realpathSync(browserPath);
    const fd = fs.openSync(diag.launcher_realpath, "r");
    const buf = Buffer.alloc(4096);
    const n = fs.readSync(fd, buf, 0, buf.length, 0);
    fs.closeSync(fd);
    diag.launcher = launcherInfo(buf.subarray(0, n).toString("latin1"));
  } catch (e) {
    diag.launcher_error = e.code ?? String(e);
  }
  if (platform !== "win32") {
    const ps = spawnSync("ps", ["-eo", "pid=,ppid=,stat=,etimes=,args="], { encoding: "utf8" });
    const tree = descendants(parsePs(ps.stdout ?? ""), pid);
    diag.process_tree = tree.map((r) => ({ ...r, args: short(r.args) }));
    if (platform === "linux") {
      // Which of the standard streams and the DevTools pipe (3 = commands in, 4 = answers out)
      // each process actually holds.
      diag.fds = tree.slice(0, 8).map((r) => {
        const fds = {};
        for (const n of [0, 1, 2, 3, 4]) {
          try {
            fds[n] = fs.readlinkSync(`/proc/${r.pid}/fd/${n}`);
          } catch (e) {
            fds[n] = e.code ?? "error";
          }
        }
        return { pid: r.pid, fds };
      });
    }
  }
  try {
    const entries = fs.readdirSync(profile, { withFileTypes: true });
    diag.profile = {
      path: profile,
      entries: entries.map((e) => (e.isDirectory() ? `${e.name}/` : e.name)).slice(0, 40),
    };
    for (const name of ["SingletonLock", "DevToolsActivePort"]) {
      const p = path.join(profile, name);
      try {
        diag.profile[name] = fs.lstatSync(p).isSymbolicLink() ? fs.readlinkSync(p) : "present";
      } catch {
        diag.profile[name] = null;
      }
    }
  } catch (e) {
    diag.profile = { path: profile, error: e.code ?? String(e) };
  }
  return diag;
}
