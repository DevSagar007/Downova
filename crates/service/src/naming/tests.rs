#![allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]

use super::*;

fn cd(value: &str) -> Disposition {
    parse_content_disposition(value)
}

fn response(cd: Option<&str>, url: Option<&str>) -> ResponseNaming {
    ResponseNaming {
        content_disposition: cd.map(str::to_owned),
        url_file_name: url.map(str::to_owned),
    }
}

const WINDOWS: Limits = Limits {
    component: 255,
    measure: Measure::Utf16Units,
    path: 259,
};
const UNIX: Limits = Limits {
    component: 255,
    measure: Measure::Utf8Bytes,
    path: 4095,
};

// ------------------------------------------------------------------ RFC 6266 / 8187 parsing

#[test]
fn filename_star_wins_over_filename() {
    let d = cd("attachment; filename=\"plain.txt\"; filename*=UTF-8''%E0%A6%AC%E0%A6%87.txt");
    assert_eq!(d.filename_ext.as_deref(), Some("বই.txt"));
    assert_eq!(d.filename.as_deref(), Some("plain.txt"));
    let header = "attachment; filename*=UTF-8''%E0%A6%AC%E0%A6%87.txt; filename=\"plain.txt\"";
    let (name, source) = choose(
        Some(&response(Some(header), Some("u.bin"))),
        "http://h/r.bin",
    );
    assert_eq!((name.as_str(), source), ("বই.txt", Source::FilenameExt));
}

#[test]
fn precedence_falls_through_unusable_sources() {
    let pick = |cd: Option<&str>, url: Option<&str>| {
        choose(Some(&response(cd, url)), "http://h/req.bin?sig=SECRET")
    };
    assert_eq!(
        pick(Some("attachment; filename=\"a.txt\""), Some("u.bin")),
        ("a.txt".into(), Source::Filename)
    );
    // An unsupported charset discards filename*, so filename is used.
    assert_eq!(
        pick(
            Some("attachment; filename*=KOI8-R''%E1; filename=b.txt"),
            None
        ),
        ("b.txt".into(), Source::Filename)
    );
    // Nothing usable in the header: the final response URL.
    assert_eq!(
        pick(
            Some("attachment; filename=\"...\""),
            Some("final%20name.iso")
        ),
        ("final name.iso".into(), Source::ResponseUrl)
    );
    assert_eq!(
        pick(Some("inline"), None),
        ("req.bin".into(), Source::RequestUrl)
    );
    assert_eq!(pick(None, None), ("req.bin".into(), Source::RequestUrl));
    assert_eq!(
        choose(Some(&response(None, None)), "http://h/"),
        ("download".into(), Source::Default)
    );
    assert_eq!(
        choose(None, "http://h/x.zip"),
        ("x.zip".into(), Source::RequestUrl)
    );
}

#[test]
fn quoted_strings_keep_semicolons_and_escapes() {
    let name = |v: &str| cd(v).filename;
    assert_eq!(
        name(r#"attachment; filename="a;b.txt""#).as_deref(),
        Some("a;b.txt")
    );
    assert_eq!(
        name(r#"attachment; filename="say \"hi\".txt""#).as_deref(),
        Some("say \"hi\".txt")
    );
    assert_eq!(
        name(r#"attachment; filename="back\\slash.txt""#).as_deref(),
        Some("back\\slash.txt")
    );
    assert_eq!(
        name("attachment;filename=token.txt;size=3").as_deref(),
        Some("token.txt")
    );
    assert_eq!(
        name(r#"ATTACHMENT; FileName = "spaced.txt" "#).as_deref(),
        Some("spaced.txt")
    );
}

#[test]
fn malformed_and_duplicate_parameters_are_ignored_deterministically() {
    // Unterminated quote: that parameter and everything after it are unusable.
    assert_eq!(
        cd(r#"attachment; filename="open.txt"#),
        Disposition::default()
    );
    assert_eq!(
        cd(r#"attachment; filename*=UTF-8''ok.txt; filename="open"#)
            .filename_ext
            .as_deref(),
        Some("ok.txt")
    );
    // Junk after a closing quote.
    assert_eq!(
        cd(r#"attachment; filename="a.txt"junk; filename*=UTF-8''b.txt"#),
        Disposition::default()
    );
    // Duplicates are dropped (all occurrences); other parameters survive.
    let d = cd("attachment; filename=a.txt; filename=b.txt; filename*=UTF-8''c.txt");
    assert_eq!(
        (d.filename, d.filename_ext.as_deref()),
        (None, Some("c.txt"))
    );
    let d = cd("attachment; filename*=UTF-8''a.txt; FILENAME*=UTF-8''b.txt; filename=c.txt");
    assert_eq!(
        (d.filename_ext, d.filename.as_deref()),
        (None, Some("c.txt"))
    );
    // Bad percent-encoding, invalid UTF-8, a malformed ext-value.
    assert_eq!(cd("a; filename*=UTF-8''bad%2.txt").filename_ext, None);
    assert_eq!(cd("a; filename*=UTF-8''%FF%FE.txt").filename_ext, None);
    assert_eq!(cd("a; filename*=UTF-8%E0.txt").filename_ext, None);
    // ISO-8859-1 with a language tag.
    assert_eq!(
        cd("a; filename*=iso-8859-1'de'f%FCr.txt")
            .filename_ext
            .as_deref(),
        Some("für.txt")
    );
    // No parameters, empty names, valueless parameters.
    assert_eq!(cd("attachment"), Disposition::default());
    assert_eq!(
        cd("attachment; ; =x; flag; filename=z.txt")
            .filename
            .as_deref(),
        Some("z.txt")
    );
    // `filename` is never percent-decoded.
    assert_eq!(
        cd("a; filename=\"%41.txt\"").filename.as_deref(),
        Some("%41.txt")
    );
}

// ------------------------------------------------------------------ sanitizing

#[test]
fn separators_and_traversal_never_leave_the_directory() {
    for raw in [
        "../../etc/passwd",
        "..\\..\\Windows\\win.ini",
        "/abs/path.txt",
        "C:\\x\\y.txt",
        "a/../../b",
    ] {
        let name = sanitize(raw).unwrap();
        assert!(!name.contains(['/', '\\', ':']), "{raw} → {name}");
        assert!(!name.starts_with('.'), "{raw} → {name}");
    }
    // An encoded traversal in filename* is decoded first, then sanitized.
    let header = "a; filename*=UTF-8''..%2F..%2Fevil.sh";
    let (name, _) = choose(Some(&response(Some(header), None)), "http://h/x");
    assert_eq!(name, "_.._evil.sh");
    // An encoded separator in a URL segment.
    assert_eq!(provisional("http://h/a%2Fb%5Cc.txt").0, "a_b_c.txt");
    assert_eq!(sanitize(".."), None);
    assert_eq!(sanitize(" . . "), None);
    assert_eq!(sanitize(".hidden"), Some("hidden".into()));
}

#[test]
fn controls_reserved_names_and_trailing_dots() {
    assert_eq!(sanitize("a\u{0}b\nc\u{1F}.txt").unwrap(), "a_b_c_.txt");
    assert_eq!(
        sanitize("in<v>a:l\"i|d?*.txt").unwrap(),
        "in_v_a_l_i_d__.txt"
    );
    assert_eq!(sanitize("report.txt. . ").unwrap(), "report.txt");
    // Disguising format characters go; ZWJ stays (Bangla and emoji need it).
    assert_eq!(
        sanitize("invoice\u{202E}fdp.exe").unwrap(),
        "invoicefdp.exe"
    );
    assert_eq!(sanitize("ক্\u{200D}ষ.txt").unwrap(), "ক্\u{200D}ষ.txt");
    for reserved in [
        "CON",
        "con.txt",
        "Nul.tar.gz",
        "COM1.log",
        "lpt9",
        "CONIN$",
        "COM¹.txt",
        "aux .txt",
    ] {
        assert!(sanitize(reserved).unwrap().starts_with('_'), "{reserved}");
    }
    for fine in ["CONSOLE.txt", "COM10", "nullable.txt", "auxiliary"] {
        assert!(!sanitize(fine).unwrap().starts_with('_'), "{fine}");
    }
    assert_eq!(sanitize("x.downova-part").unwrap(), "x.downova-part_");
    assert_eq!(sanitize("x.ARIA2").unwrap(), "x.ARIA2_");
}

#[test]
fn unicode_is_preserved_and_normalized() {
    assert_eq!(sanitize("বাংলা বই ২০২৬.pdf").unwrap(), "বাংলা বই ২০২৬.pdf");
    assert_eq!(sanitize("👩‍👩‍👧 family.png").unwrap(), "👩‍👩‍👧 family.png");
    // A decomposed é becomes the composed form.
    assert_eq!(sanitize("cafe\u{301}.txt").unwrap(), "caf\u{e9}.txt");
    assert_eq!(key("CAFE\u{301}.TXT"), key("café.txt"));
}

// ------------------------------------------------------------------ length limits

fn long(unit: &str, n: usize, ext: &str) -> String {
    format!("{}{ext}", unit.repeat(n))
}

/// Byte offsets where a grapheme of `s` ends.
fn grapheme_ends(s: &str) -> Vec<usize> {
    s.grapheme_indices(true).map(|(i, g)| i + g.len()).collect()
}

#[test]
fn long_names_fit_with_suffix_headroom_and_keep_the_extension() {
    let dir = Path::new("d");
    for limits in [WINDOWS, UNIX] {
        let m = limits.measure;
        for name in [
            long("a", 400, ".zip"),
            long("বাংলা", 200, ".pdf"),
            long("👩‍👩‍👧", 100, ".png"),
            long("ক্ষ", 150, ".gz"),
        ] {
            let fitted = fit(&name, dir, limits);
            let worst = format!("{fitted} (deadbeef).downova-part");
            assert!(
                m.of(&worst) <= limits.component,
                "{limits:?}: {}",
                m.of(&worst)
            );
            let ext = name.rsplit_once('.').unwrap().1;
            let stem = fitted.strip_suffix(&format!(".{ext}")).unwrap();
            // The kept stem is a prefix of whole graphemes: nothing was split.
            assert!(name.starts_with(stem));
            assert!(grapheme_ends(&name).contains(&stem.len()), "{stem}");
        }
    }
    // Bangla takes 3 UTF-8 bytes but 1 UTF-16 unit per letter: Windows keeps 3× as many letters.
    let bangla = long("ব", 300, ".pdf");
    assert_eq!(WINDOWS.measure.of(&fit(&bangla, dir, WINDOWS)), 231);
    assert!(UNIX.measure.of(&fit(&bangla, dir, UNIX)) <= 231);
    assert!(fit(&bangla, dir, UNIX).chars().count() < 80);
    // Short names are untouched.
    assert_eq!(fit("a.zip", dir, WINDOWS), "a.zip");
}

#[test]
fn long_directories_shorten_the_name_down_to_a_floor() {
    let deep = format!("C:\\{}", "x".repeat(150));
    let fitted = fit(&long("n", 200, ".iso"), Path::new(&deep), WINDOWS);
    let total = WINDOWS
        .measure
        .of(&format!("{deep}\\{fitted} (deadbeef).downova-part"));
    assert!(total <= 259, "{total}");
    // A directory beyond the path limit leaves the 32-unit floor (long paths take over).
    let deeper = format!("C:\\{}", "x".repeat(300));
    assert_eq!(
        WINDOWS
            .measure
            .of(&fit(&long("n", 200, ".iso"), Path::new(&deeper), WINDOWS)),
        32
    );
    // An over-long extension-like tail is cut as part of the stem.
    let weird = format!("a.{}", "e".repeat(300));
    assert!(UNIX.measure.of(&fit(&weird, Path::new("d"), UNIX)) <= 231);
}

// ------------------------------------------------------------------ collisions

#[test]
fn collisions_with_files_jobs_and_partial_files_get_a_number() {
    let dir = tempfile::tempdir().unwrap();
    let d = dir.path();
    let none = HashSet::new();
    std::fs::write(d.join("a.zip"), b"user").unwrap();
    assert_eq!(unique(d, "a.zip", &none, None, UNIX), "a (1).zip");
    // Another job's partial or aria2 control file blocks the name too...
    std::fs::write(d.join("b.zip.downova-part"), b"p").unwrap();
    std::fs::write(d.join("c.zip.aria2"), b"p").unwrap();
    assert_eq!(unique(d, "b.zip", &none, None, UNIX), "b (1).zip");
    assert_eq!(unique(d, "c.zip", &none, None, UNIX), "c (1).zip");
    // ...but not the job's own partial file.
    assert_eq!(unique(d, "b.zip", &none, Some("b.zip"), UNIX), "b.zip");
    // Other jobs' names compare case- and normalization-insensitively.
    let taken: HashSet<String> = [key("REPORT.PDF"), key("cafe\u{301}.txt")].into();
    assert_eq!(
        unique(d, "report.pdf", &taken, None, UNIX),
        "report (1).pdf"
    );
    assert_eq!(unique(d, "café.txt", &taken, None, UNIX), "café (1).txt");
    // Existing files are never touched.
    assert_eq!(std::fs::read(d.join("a.zip")).unwrap(), b"user");
}
