import { describe, expect, it } from "vitest";
import { MAX_BATCH, matchesFilter, parseLinks, previewLink } from "./batch";

describe("previewLink and matchesFilter (F04)", () => {
  it("shows the decoded last segment and the site, or no name", () => {
    expect(previewLink("https://example.org:8443/a/%E0%A6%AC%20x.zip?sig=1")).toEqual({
      name: "ব x.zip",
      site: "example.org:8443",
    });
    expect(previewLink("https://example.org/dir/")).toEqual({ name: null, site: "example.org" });
    expect(previewLink("ftp://ftp.example.net/%E0%A6.bin").name).toBe("%E0%A6.bin");
  });

  it("filters by link, name or site, ignoring case", () => {
    const link = "https://Mirror.example.org/pub/Big%20File.ISO";
    const p = previewLink(link);
    expect(matchesFilter(link, p, "")).toBe(true);
    expect(matchesFilter(link, p, " big file ")).toBe(true);
    expect(matchesFilter(link, p, "mirror.EXAMPLE")).toBe(true);
    expect(matchesFilter(link, p, ".zip")).toBe(false);
  });
});

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
    const r = parseLinks("scp://example.org/x.bin\nplease download\nhttps://user:pw@example.org/y");
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
