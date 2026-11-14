import { describe, expect, it } from "vitest";
import { clipboardOffers, readDrop, type MonitorMemory } from "./intake";

function transfer(data: Record<string, string>, files: File[] = []) {
  const types = [...Object.keys(data), ...(files.length ? ["Files"] : [])];
  return {
    types,
    getData: (t: string) => data[t] ?? "",
    files: files as unknown as FileList,
  };
}

describe("readDrop (F03)", () => {
  it("takes links from a uri-list, skipping comments", () => {
    expect(
      readDrop(
        transfer({
          "text/uri-list": "# from a page\r\nhttps://e.org/a.zip\r\nhttps://e.org/b.iso",
        }),
      ),
    ).toEqual({ kind: "links", links: ["https://e.org/a.zip", "https://e.org/b.iso"] });
  });

  it("falls back to links in plain text, then to one list file", () => {
    expect(readDrop(transfer({ "text/plain": "see https://e.org/c.pdf please" }))).toEqual({
      kind: "links",
      links: ["https://e.org/c.pdf"],
    });
    const list = new File(["https://e.org/x\n"], "links.txt", { type: "text/plain" });
    expect(readDrop(transfer({}, [list]))).toEqual({ kind: "listFile", file: list });
  });

  it("reports anything else as unsupported", () => {
    const image = new File(["x"], "photo.png", { type: "image/png" });
    expect(readDrop(transfer({}, [image]))).toEqual({ kind: "unsupported" });
    expect(readDrop(transfer({ "text/plain": "no links here" }))).toEqual({ kind: "unsupported" });
    const two = [new File(["a"], "a.txt"), new File(["b"], "b.txt")];
    expect(readDrop(transfer({}, two))).toEqual({ kind: "unsupported" });
  });
});

describe("clipboardOffers (F02)", () => {
  const fresh = (): MonitorMemory => ({ lastText: null, offered: new Set() });

  it("offers only new http(s) file links, once, and never for unchanged text", () => {
    const m = fresh();
    const text = "https://e.org/setup.exe https://e.org/page https://e.org/a.zip";
    expect(clipboardOffers(text, m, [])).toEqual([
      "https://e.org/setup.exe",
      "https://e.org/a.zip",
    ]);
    expect(clipboardOffers(text, m, [])).toEqual([]);
    expect(clipboardOffers(`${text}\nhttps://e.org/new.iso`, m, [])).toEqual([
      "https://e.org/new.iso",
    ]);
  });

  it("follows ignored sites and ignores empty or non-link text", () => {
    const m = fresh();
    expect(clipboardOffers("https://Ads.Example.org/x.zip", m, ["ads.example.org"])).toEqual([]);
    expect(clipboardOffers(null, m, [])).toEqual([]);
    expect(clipboardOffers("just words", m, [])).toEqual([]);
    expect(clipboardOffers("sftp://e.org/a.zip", m, [])).toEqual([]);
  });
});
