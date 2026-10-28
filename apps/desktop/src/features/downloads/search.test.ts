import { describe, expect, it } from "vitest";
import { matchesSearch, normalizeSearchText, sourceHostname } from "./search";
import { snapshot } from "../../test/fake-service";

describe("history search", () => {
  it("matches the display name case-insensitively with Unicode normalization", () => {
    const accented = snapshot({ display_name: "RÉSUMÉ.PDF" });
    expect(matchesSearch(accented, "résumé")).toBe(true);
    expect(matchesSearch(accented, "RE\u0301SUME\u0301")).toBe(true);
    expect(matchesSearch(snapshot({ display_name: "ＦＩＬＥ.zip" }), "file")).toBe(true);
    expect(matchesSearch(snapshot({ display_name: "report.pdf" }), "  REPORT  ")).toBe(true);
  });

  it("matches Bangla and right-to-left names without treating them specially", () => {
    expect(matchesSearch(snapshot({ display_name: "বাংলা-নোট.pdf" }), "বাংলা")).toBe(true);
    expect(matchesSearch(snapshot({ display_name: "প্রতিবেদন.docx" }), "প্রতিবেদন")).toBe(true);
    expect(matchesSearch(snapshot({ display_name: "تقرير-عربي.pdf" }), "تقرير")).toBe(true);
  });

  it("matches the source hostname only, never its path, query or credentials", () => {
    const download = snapshot({
      display_name: "archive.zip",
      source_display:
        "https://user:pass@files.example.org/private/token-value/file.zip?token=hidden#frag",
    });
    expect(matchesSearch(download, "files.example.org")).toBe(true);
    expect(matchesSearch(download, "EXAMPLE.ORG")).toBe(true);
    expect(matchesSearch(download, "archive")).toBe(true);
    expect(matchesSearch(download, "private")).toBe(false);
    expect(matchesSearch(download, "token-value")).toBe(false);
    expect(matchesSearch(download, "hidden")).toBe(false);
    expect(matchesSearch(download, "user")).toBe(false);
    expect(matchesSearch(download, "frag")).toBe(false);
  });

  it("treats a blank query as no filter and ignores malformed sources", () => {
    const download = snapshot({ display_name: "a.bin", source_display: "not a url" });
    expect(matchesSearch(download, "")).toBe(true);
    expect(matchesSearch(download, "   ")).toBe(true);
    expect(sourceHostname("not a url")).toBe("");
    expect(matchesSearch(download, "not a url")).toBe(false);
  });

  it("normalises compatibility forms and keeps a trailing dot out of the hostname", () => {
    expect(normalizeSearchText("ﬁle")).toBe("file");
    expect(sourceHostname("https://EXAMPLE.org./file")).toBe("example.org");
  });
});
