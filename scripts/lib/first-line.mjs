// Reads the first complete line a child process writes to stdout (B2.33).
//
// A fixture announces itself with one JSON line. Reading the first `data` event instead is wrong
// in three ways: stdout may arrive in several chunks (half a line), the process may exit before
// writing anything, and nothing bounds the wait. This helper buffers chunks until a newline,
// rejects when the stream ends or the process exits first, rejects after `timeoutMs`, and removes
// its listeners in every case, so later output is not consumed or leaked.
import { StringDecoder } from "node:string_decoder";

/**
 * @param {{ stdout: import("node:stream").Readable, once?: Function, off?: Function }} child
 *   a ChildProcess (or anything with `stdout` and the `exit` event)
 * @param {{ timeoutMs?: number, maxBytes?: number }} [opts]
 * @returns {Promise<string>} the first line, without its line ending
 */
export function readFirstLine(child, { timeoutMs = 30_000, maxBytes = 1 << 20 } = {}) {
  return new Promise((resolve, reject) => {
    let buf = "";
    // A multi-byte character split across chunks is joined, not mangled.
    const decoder = new StringDecoder("utf8");
    let exited = null;
    let grace = null;
    const done = (fn, v) => {
      clearTimeout(timer);
      clearTimeout(grace);
      child.stdout.off("data", onData);
      child.stdout.off("end", onEnd);
      child.stdout.off("error", onError);
      child.off?.("exit", onExit);
      fn(v);
    };
    const onData = (chunk) => {
      buf += typeof chunk === "string" ? chunk : decoder.write(chunk);
      const nl = buf.indexOf("\n");
      if (nl >= 0) done(resolve, buf.slice(0, nl).replace(/\r$/, ""));
      else if (buf.length > maxBytes)
        done(reject, new Error(`no line within the first ${maxBytes} bytes`));
    };
    const onEnd = () =>
      done(
        reject,
        new Error(
          `stdout ended before a complete line${exited ? ` (process exited ${exited})` : ""}; got ${JSON.stringify(buf.slice(0, 200))}`,
        ),
      );
    const onError = (e) => done(reject, e);
    // `exit` can fire before the last stdout chunk is read: let `end` decide, with a short
    // grace in case a grandchild keeps the pipe open.
    const onExit = (code, signal) => {
      exited = String(code ?? signal);
      grace = setTimeout(
        () => done(reject, new Error(`process exited (${exited}) before a complete line`)),
        2000,
      );
    };
    const timer = setTimeout(
      () => done(reject, new Error(`no complete line within ${timeoutMs} ms`)),
      timeoutMs,
    );
    child.stdout.on("data", onData);
    child.stdout.once("end", onEnd);
    child.stdout.once("error", onError);
    child.once?.("exit", onExit);
  });
}

/** `readFirstLine` parsed as JSON; a malformed line rejects with the line in the message. */
export async function readFirstJsonLine(child, opts) {
  const line = await readFirstLine(child, opts);
  try {
    return JSON.parse(line);
  } catch {
    throw new Error(`the first line is not JSON: ${JSON.stringify(line.slice(0, 200))}`);
  }
}
