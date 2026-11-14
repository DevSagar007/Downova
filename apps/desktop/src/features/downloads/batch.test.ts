import { describe, expect, it } from "vitest";
import { MAX_BATCH, parseLinks } from "./batch";

describe("parseLinks", () => {
  it("takes http(s) links from lines and spaces, keeps signed queries, drops repeats", () => {
    const text = [
      "  https://example.org/a.zip  ",
      "http://example.org/b.iso https://example.org/c?sig=Zz%3D%3D&x=1",
      "<https://example.org/d.pdf>,",
      "https://example.org/a.zip",
      "",
    ].join("\r\n");
    expect(parseLinks(text)).toEqual({
      links: [
        "https://example.org/a.zip",
        "http://example.org/b.iso",
        "https://example.org/c?sig=Zz%3D%3D&x=1",
        "https://example.org/d.pdf",
      ],
      ignored: 0,
      overflow: 0,
    });
  });

  it("counts what it cannot add instead of guessing", () => {
    const r = parseLinks(
      "sftp://example.org/x.bin\nplease download\nhttps://user:pw@example.org/y",
    );
    expect(r.links).toEqual([]);
    expect(r.ignored).toBe(4);
    // F05: anonymous FTP links are links; FTP with credentials is not.
    expect(parseLinks("ftp://example.org/x.bin ftp://u:p@example.org/y").links).toEqual([
      "ftp://example.org/x.bin",
    ]);
  });

  it("reports links beyond the batch limit", () => {
    const many = Array.from({ length: MAX_BATCH + 3 }, (_, i) => `https://example.org/${i}`);
    const r = parseLinks(many.join("\n"));
    expect(r.links).toHaveLength(MAX_BATCH);
    expect(r.overflow).toBe(3);
  });
});
