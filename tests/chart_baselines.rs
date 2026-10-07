#![cfg(any(target_os = "linux", target_os = "windows"))]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::{fs, io::ErrorKind};

const BIN: &str = env!("CARGO_BIN_EXE_itgmania-harness-rs");

mod support;

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        loop {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!("itg-chart-test-{}-{id}", std::process::id()));
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create chart-test workspace: {error}"),
            }
        }
    }
    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        path
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn run(args: &[&Path]) -> Output {
    Command::new(BIN).args(args).output().unwrap()
}
fn document(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
fn theme() -> PathBuf {
    support::theme_root()
}
fn sm(description: &str, difficulty: &str, notes: &str) -> String {
    format!(
        "#TITLE:Chart capture regression;\n#BPMS:0=150.125;\n#NOTES:dance-single:{description}:{difficulty}:10:0,0,0,0,0:\n{notes}\n;\n"
    )
}
fn baseline(manifest: &Path, out: &Path) -> Output {
    run(&[Path::new("baseline"), manifest, Path::new("--out"), out])
}

#[test]
fn rssp_export_preserves_raw_capture_and_legacy_metadata_policy() {
    let work = Workspace::new();
    let mut bytes = sm("Test", "Challenge", "1000\n0000\n0100\n0000").into_bytes();
    bytes.extend_from_slice(b"#ARTIST: Kr\xe9 ;\n");
    let path = work.0.join("legacy.sm");
    fs::write(&path, &bytes).unwrap();
    let capture = work.0.join("native.json.zst");
    let references = work.0.join("references");
    let summary = document(run(&[Path::new("charts"), &path, Path::new("--theme"), &theme(),
        Path::new("--out"), &capture, Path::new("--rssp-baseline"), &references]));
    assert!(summary["rssp_error"].is_null());
    let digest = format!("{:x}", md5::compute(&bytes));
    assert_eq!(summary["source_md5"], digest);
    let read = |p: &Path| -> Value {
        serde_json::from_slice(&zstd::decode_all(fs::read(p).unwrap().as_slice()).unwrap()).unwrap()
    };
    let raw = read(&capture);
    let report = read(&references.join(&digest[..2]).join(format!("{digest}.json.zst")));
    assert_eq!(raw["charts"][0]["artist"]["raw_bytes_hex"], "204b72e920");
    assert_eq!(report[0]["artist"], "Kré");
    assert_eq!(report[0]["artist_translated"], "Kré");
    assert_eq!(report[0]["notes_per_measure"], serde_json::json!([2]));
    assert_eq!(report[0]["peak_nps"], 1.25104);
    assert_eq!(report[0]["hash"], raw["charts"][0]["theme"]["players"][0]["streams"]["Hash"]);
}

#[test]
fn legacy_theme_uses_native_timing_with_lua_double_precision() {
    let work = Workspace::new();
    let theme = work.0.join("theme");
    fs::create_dir_all(theme.join("Scripts")).unwrap();
    fs::write(theme.join("Scripts/SL-ChartParser.lua"), r#"
local GetSimfileString = function(steps) return _READ_FILE(steps:GetFilename()), 'sm' end
local GetSimfileChartString = function(bytes, ...) return bytes, '0.000=123.000' end
function ParseChartInfo(steps, pn)
    local bytes, ext = GetSimfileString(steps)
    local notes, bpms = GetSimfileChartString(bytes, ext)
    assert(notes and bpms)
    assert(steps:CalculateTechCounts(player):GetValue('TechCountsCategory_Crossovers') >= 0)
    local td = steps:GetTimingData()
    local duration = td:GetElapsedTimeFromBeat(80) - td:GetElapsedTimeFromBeat(76)
    SL[pn].Streams = {Hash='0123456789abcdef', NotesPerMeasure={1},
        NPSperMeasure={1/duration}, PeakNPS=1/duration, EquallySpacedPerMeasure={true}}
end
"#).unwrap();
    fs::write(theme.join("Scripts/SL-ChartParserHelpers.lua"), r#"
function GetStreamSequences() return {} end
function GenerateBreakdownText() return 'No Streams!' end
function GetTotalStreamAndBreakMeasures() return 0, 1 end
"#).unwrap();
    fs::write(theme.join("Scripts/SL-BPMDisplayHelpers.lua"), r#"
function GetDisplayBPMs(player, steps) assert(not GAMESTATE:IsCourseMode()); return steps:GetDisplayBpms() end
function StringifyDisplayBPMs() return '123' end
"#).unwrap();
    let notes = vec!["1000\n0000\n0000\n0000"; 20].join("\n,\n");
    let path = work.write("precision.sm", &format!("#TITLE:Precision;\n#OFFSET:-0.478;\n#BPMS:0=123;\n#NOTES:dance-single:Test:Challenge:10:0,0,0,0,0:\n{notes}\n;"));
    let doc = document(run(&[Path::new("charts"), &path, Path::new("--theme"), &theme]));
    let chart = &doc["charts"][0];
    assert_eq!(chart["theme"]["status"], "complete");
    let lua_nps = chart["theme"]["players"][0]["streams"]["NPSperMeasure"][0].as_f64().unwrap();
    let native_nps = f64::from(chart["player_stats"][0]["nps_per_measure"][19].as_f64().unwrap() as f32);
    assert_eq!(format!("{lua_nps:.5e}"), "5.12499e-1");
    assert_eq!(format!("{native_nps:.5e}"), "5.12500e-1");
}

#[test]
fn incomplete_theme_cannot_export_or_retain_a_stale_rssp_baseline() {
    let work = Workspace::new();
    let text = sm("Tech Rating\\: 4/10", "Challenge", "1000\n0000\n0100\n0000");
    let path = work.write("partial.sm", &text);
    let digest = format!("{:x}", md5::compute(text.as_bytes()));
    let references = work.0.join("references");
    let dir = references.join(&digest[..2]);
    fs::create_dir_all(&dir).unwrap();
    let stale = dir.join(format!("{digest}.json.zst"));
    fs::write(&stale, b"stale").unwrap();
    let capture = work.0.join("native.json.zst");
    let summary = document(run(&[Path::new("charts"), &path, Path::new("--theme"), &theme(),
        Path::new("--out"), &capture, Path::new("--rssp-baseline"), &references]));
    assert!(summary["rssp_error"].as_str().unwrap().contains("incomplete theme"));
    assert!(capture.exists());
    assert!(!stale.exists());
}

#[test]
fn theme_only_isolates_keep_original_engine_values_and_chart_identity() {
    let work = Workspace::new();
    let first = sm("Same", "Challenge", "1000\n0000\n0100\n0000");
    let second = sm("Other", "Challenge", "0001\n0000\n0010\n0000");
    let second_block = &second[second.find("#NOTES:").unwrap()..];
    let original = work.write("original.sm", &(first.clone()+second_block));
    work.write("first.sm", &first);
    work.write("second.sm", &second);
    let sha = |s: &[u8]| {
        use sha2::{Digest, Sha256};
        Sha256::digest(s).iter().map(|b| format!("{b:02x}")).collect::<String>()
    };
    let source_sha = sha(&fs::read(&original).unwrap());
    let cases = serde_json::json!({"version":1,"cases":[
        {"source_sha256":source_sha,"source_index":0,"oracle_simfile":"first.sm","oracle_index":0,
         "oracle_sha256":sha(first.as_bytes()),"reason":"isolate the first original chart"},
        {"source_sha256":source_sha,"source_index":1,"oracle_simfile":"second.sm","oracle_index":0,
         "oracle_sha256":sha(second.as_bytes()),"reason":"isolate the second original chart"}
    ]});
    let manifest = work.write("repairs.json", &cases.to_string());
    let raw = document(run(&[Path::new("charts"), &original, Path::new("--theme"), &theme()]));
    let repaired = document(run(&[Path::new("charts"), &original, Path::new("--theme"), &theme(),
        Path::new("--theme-repairs"), &manifest]));
    assert_eq!(raw["source_sha256"], repaired["source_sha256"]);
    assert_eq!(repaired["theme_repairs"].as_array().unwrap().len(), 2);
    for index in 0..2 {
        assert_eq!(repaired["charts"][index]["theme"]["status"], "complete");
        for field in ["source_index","description","credit","note_data","timing","bpm","player_stats","groove_stats_hash","first_second","last_second"] {
            assert_eq!(raw["charts"][index][field], repaired["charts"][index][field], "{field}");
        }
    }
    // A pinned copy containing different notes is still rejected by the engine proof.
    let changed = sm("Same", "Challenge", "1100\n0000\n0110\n0000");
    work.write("changed.sm", &changed);
    let bad = work.write("changed.json", &serde_json::json!({"version":1,"cases":[
        {"source_sha256":source_sha,"source_index":0,"oracle_simfile":"changed.sm","oracle_index":0,
         "oracle_sha256":sha(changed.as_bytes()),"reason":"must fail: changed notes"}
    ]}).to_string());
    let rejected = run(&[Path::new("charts"), &original, Path::new("--theme"), &theme(),
        Path::new("--theme-repairs"), &bad]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("changes engine chart data"));
    let mut unpinned = cases;
    unpinned["cases"][0]["oracle_sha256"] = serde_json::json!("0".repeat(64));
    let bad = work.write("unpinned.json", &unpinned.to_string());
    let rejected = run(&[Path::new("charts"), &original, Path::new("--theme"), &theme(),
        Path::new("--theme-repairs"), &bad]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("SHA256 does not match"));
}

#[test]
fn compressed_capture_preserves_native_document_and_reports_partial_theme() {
    let work = Workspace::new();
    let path = work.write(
        "capture.sm",
        &sm("Test", "Challenge", "1100\n0000\n1100\n0000"),
    );
    let expected = document(run(&[
        Path::new("charts"),
        &path,
        Path::new("--theme"),
        &theme(),
    ]));
    let output = work.0.join("capture.json.zst");
    let summary = document(run(&[
        Path::new("charts"),
        &path,
        Path::new("--theme"),
        &theme(),
        Path::new("--out"),
        &output,
    ]));
    let bytes = zstd::decode_all(fs::read(output).unwrap().as_slice()).unwrap();
    let captured: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(captured, expected);
    assert_eq!(summary["charts"], 1);
    assert_eq!(summary["source_sha256"], expected["source_sha256"]);
    assert_eq!(expected["charts"][0]["hash_bpms"], "0.000=150.125");
    assert_eq!(expected["charts"][0]["player_stats"][0]["radar"][5], 4.0);
    assert_eq!(expected["charts"][0]["player_stats"][0]["radar"][6], 2.0);
}

#[test]
fn unicode_filenames_preserve_source_bytes_and_chart_values() {
    let work = Workspace::new();
    let text = sm("Test", "Challenge", "1000\n0000\n0100\n0000");
    let ascii = work.write("chart.sm", &text);
    let unicode = work.write("BYØRN ☆ chart.sm", &text);
    let capture = |path: &Path| {
        document(run(&[
            Path::new("charts"),
            path,
            Path::new("--theme"),
            &theme(),
        ]))
    };
    let a = capture(&ascii);
    let b = capture(&unicode);
    assert_eq!(a["source_sha256"], b["source_sha256"]);
    assert_eq!(a["charts"], b["charts"]);
}

#[test]
fn non_utf8_metadata_preserves_exact_bytes_and_valid_chart_references() {
    let work = Workspace::new();
    let mut bytes = sm("Test", "Challenge", "1000\n0000\n0100\n0000").into_bytes();
    bytes.extend_from_slice(b"#ARTIST:Kr\xe9;\n");
    let path = work.0.join("legacy.sm");
    fs::write(&path, bytes).unwrap();
    let doc = document(run(&[
        Path::new("charts"),
        &path,
        Path::new("--theme"),
        &theme(),
    ]));
    let chart = &doc["charts"][0];
    assert_eq!(chart["metadata_utf8"], false);
    assert_eq!(chart["artist"]["raw_bytes_hex"], "4b72e9");
    assert_eq!(chart["groove_stats_hash"].as_str().unwrap().len(), 16);
    assert_eq!(chart["theme"]["status"], "complete");
}

#[test]
fn unsupported_extra_types_do_not_block_supported_charts_or_create_fake_stats() {
    let work = Workspace::new();
    let text = sm("Supported", "Challenge", "1000\n0000\n0100\n0000");
    let file = work.write(
        "mixed.sm",
        &(text.clone() + &text.replace("dance-single", "para-versus")),
    );
    let doc = document(run(&[
        Path::new("charts"),
        &file,
        Path::new("--theme"),
        &theme(),
    ]));
    assert_eq!(doc["charts"].as_array().unwrap().len(), 2);
    assert_eq!(doc["charts"][0]["note_data_supported"], true);
    assert_eq!(doc["charts"][0]["theme"]["status"], "complete");
    assert_eq!(doc["charts"][1]["note_data_supported"], false);
    assert_eq!(doc["charts"][1]["player_stats"], serde_json::json!([]));
    assert_eq!(doc["charts"][1]["first_second"], Value::Null);
    assert_eq!(doc["charts"][1]["last_second"], Value::Null);
    assert_eq!(doc["charts"][1]["theme"]["status"], "unavailable");
}

#[test]
fn captures_native_stats_and_theme_display_without_rounding_native_values() {
    let work = Workspace::new();
    let path = work.write(
        "expert.sm",
        &sm("Alias", "Expert", "1000\n0000\n0100\n0000"),
    );
    let doc = document(run(&[
        Path::new("charts"),
        &path,
        Path::new("--theme"),
        &theme(),
    ]));
    let chart = &doc["charts"][0];
    assert_eq!(chart["difficulty_code"], 4);
    assert_eq!(chart["bpm"]["actual_min"], 150.125);
    assert_eq!(
        chart["player_stats"][0]["notes_per_measure"],
        serde_json::json!([2])
    );
    assert_eq!(chart["groove_stats_hash"].as_str().unwrap().len(), 16);
    assert_eq!(chart["theme"]["status"], "complete");
    assert_eq!(
        chart["theme"]["players"][0]["streams"]["Difficulty"],
        "Challenge"
    );
    assert_eq!(chart["theme"]["display_bpm_text"], "150");
    assert_eq!(
        chart["theme"]["players"][0]["streams"]["Hash"],
        chart["groove_stats_hash"]
    );
    assert_eq!(doc["theme"]["scripts"].as_object().unwrap().len(), 3);
    assert_eq!(doc["source_sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn duplicate_edits_keep_distinct_native_notes_and_reject_ambiguous_theme_baselines() {
    let work = Workspace::new();
    let first = sm("Same", "Edit", "1000\n0000\n0100\n0000");
    let second = "#NOTES:dance-single:Same:Edit:11:0,0,0,0,0:\n0001\n0000\n0010\n0000\n;";
    let path = work.write("duplicate.sm", &(first + second));
    let doc = document(run(&[
        Path::new("charts"),
        &path,
        Path::new("--theme"),
        &theme(),
    ]));
    assert_eq!(doc["charts"][1]["source_index"], 1);
    assert_ne!(
        doc["charts"][0]["groove_stats_hash"],
        doc["charts"][1]["groove_stats_hash"]
    );
    assert_ne!(doc["charts"][0]["note_data"], doc["charts"][1]["note_data"]);
    assert_eq!(doc["charts"][1]["theme"]["status"], "partial");
    assert!(
        doc["charts"][1]["theme"]["errors"]
            .to_string()
            .contains("multiple native charts")
    );
    let ambiguous = run(&[Path::new("chart"), &path]);
    assert!(!ambiguous.status.success());
    work.write("valid.sm", &sm("Valid", "Hard", "1000\n0000\n0100\n0000"));
    let manifest = work.write("cases.toml", &format!("version=2\ntheme='{}'\n[[case]]\nname='valid'\nsimfile='valid.sm'\nall_charts=true\nexpected_chart_count=1\n[[case]]\nname='duplicate'\nsimfile='duplicate.sm'\nall_charts=true\nexpected_chart_count=2\n", theme().to_string_lossy().replace('\\', "/")));
    let out = work.0.join("baselines");
    let output = baseline(&manifest, &out);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("theme capture is"));
    assert!(!out.exists());
}

#[test]
fn malformed_source_never_receives_a_fallback_theme_hash() {
    let work = Workspace::new();
    for (name, text) in [
        (
            "colon.sm",
            sm("Tech Rating\\: 4/10", "Challenge", "1000\n0000\n0100\n0000"),
        ),
        (
            "bpm.sm",
            sm("Valid", "Challenge", "1000\n0000\n0100\n0000")
                .replace("#BPMS:0=150.125;", "#BPMS:0=150.125\n#STOPS:;"),
        ),
    ] {
        let path = work.write(name, &text);
        let doc = document(run(&[
            Path::new("charts"),
            &path,
            Path::new("--theme"),
            &theme(),
        ]));
        assert_eq!(doc["charts"].as_array().unwrap().len(), 1);
        assert_eq!(
            doc["charts"][0]["groove_stats_hash"]
                .as_str()
                .unwrap()
                .len(),
            16
        );
        assert_eq!(doc["charts"][0]["theme"]["status"], "partial");
        assert_ne!(
            doc["charts"][0]["theme"]["players"][0]["streams"]["Hash"],
            doc["charts"][0]["groove_stats_hash"]
        );
    }
    // SL scans earlier SM charts before selecting its target. The engine's
    // invalid difficulty enum is nil, so an escaped description can also
    // abort hashing a later, otherwise valid chart.
    let escaped = sm("Tech Rating\\: 4/10", "Challenge", "1000\n0000\n0100\n0000");
    let double = "#NOTES:dance-double:Valid double:Challenge:12:0,0,0,0,0:\n10000000\n00000000\n00001000\n00000000\n;";
    let path = work.write("escaped-double.sm", &(escaped + double));
    let doc = document(run(&[
        Path::new("charts"),
        &path,
        Path::new("--theme"),
        &theme(),
    ]));
    assert_eq!(doc["charts"].as_array().unwrap().len(), 2);
    assert_eq!(doc["charts"][1]["steps_type"]["source"], "dance-double");
    assert_eq!(doc["charts"][1]["theme"]["status"], "partial");
    assert!(
        doc["charts"][1]["theme"]["errors"]
            .to_string()
            .contains("string expected, got nil")
    );
    let ssc = "#VERSION:0.83;\n#TITLE:Invalid difficulty;\n#BPMS:0=125;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DESCRIPTION:Artist;\n#DIFFICULTY:XO FS DS;\n#METER:10;\n#NOTES:\n1000\n0000\n0100\n0000\n;";
    let path = work.write("difficulty.ssc", ssc);
    let doc = document(run(&[
        Path::new("charts"),
        &path,
        Path::new("--theme"),
        &theme(),
    ]));
    assert_eq!(doc["charts"][0]["difficulty_code"], 3);
    assert_eq!(doc["charts"][0]["theme"]["status"], "partial");
    assert_eq!(
        doc["charts"][0]["theme"]["players"][0]["streams"]["Hash"],
        ""
    );
}

#[test]
fn duplicate_non_edits_are_ambiguous_even_with_distinct_descriptions() {
    let work = Workspace::new();
    let source = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw-values.ssc"),
    )
    .unwrap();
    let path = work.write(
        "duplicate.ssc",
        &source.replace("#DIFFICULTY:Easy;", "#DIFFICULTY:Challenge;"),
    );
    let doc = document(run(&[
        Path::new("chart"),
        &path,
        Path::new("--index"),
        Path::new("1"),
        Path::new("--theme"),
        &theme(),
    ]));
    assert_eq!(doc["charts"][0]["description"], "Second chart");
    assert_eq!(doc["charts"][0]["source_index"], 1);
    assert_eq!(doc["charts"][0]["theme"]["status"], "partial");
    assert!(
        doc["charts"][0]["theme"]["errors"]
            .to_string()
            .contains("multiple native charts")
    );
}

#[test]
fn compressed_inputs_match_original_capture_and_validate_chart_coverage() {
    let work = Workspace::new();
    let text = sm("Original", "Hard", "1000\n0000\n0100\n0000");
    let path = work.write("song.sm", &text);
    let compressed = work.0.join("song.sm.zst");
    fs::write(
        &compressed,
        zstd::stream::encode_all(text.as_bytes(), 1).unwrap(),
    )
    .unwrap();
    let plain = document(run(&[
        Path::new("charts"),
        &path,
        Path::new("--theme"),
        &theme(),
    ]));
    let zipped = document(run(&[
        Path::new("charts"),
        &compressed,
        Path::new("--theme"),
        &theme(),
    ]));
    assert_eq!(plain["source_sha256"], zipped["source_sha256"]);
    assert_eq!(plain["charts"], zipped["charts"]);
    let manifest = work.write("cases.toml", "version=2\n[[case]]\nname='coverage'\nsimfile='song.sm.zst'\nall_charts=true\nexpected_chart_count=2\n");
    let out = work.0.join("baselines");
    let rejected = baseline(&manifest, &out);
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("expected 2 charts, ITGmania loaded 1")
    );
    assert!(!out.exists());
    work.write("empty.sm", "#TITLE:No charts;\n#BPMS:0=150;");
    assert!(
        !run(&[Path::new("charts"), &work.0.join("empty.sm")])
            .status
            .success()
    );
}

#[test]
fn records_pinned_oracle_repairs_and_verifies_without_rewriting_baselines() {
    let work = Workspace::new();
    let fixed = sm("Valid", "Challenge", "1000\n0000\n0100\n0000");
    let original = fixed.replace("#BPMS:0=150.125;", "#BPMS:0=150.125\n#STOPS:;");
    let original_path = work.write("original.sm", &original);
    let fixed_path = work.write("oracle.sm", &fixed);
    let original_doc = document(run(&[Path::new("charts"), &original_path]));
    let fixed_doc = document(run(&[Path::new("charts"), &fixed_path]));
    let text = format!(
        "version=2\ntheme='{}'\n[[case]]\nname='repaired'\nsimfile='original.sm'\noracle_simfile='oracle.sm'\nrepair_reason='Restore missing BPM terminator for oracle only'\noriginal_sha256='{}'\noracle_sha256='{}'\nall_charts=true\nexpected_chart_count=1\n",
        theme().to_string_lossy().replace('\\', "/"),
        original_doc["source_sha256"].as_str().unwrap(),
        fixed_doc["source_sha256"].as_str().unwrap()
    );
    let manifest = work.write("cases.toml", &text);
    let out = work.0.join("baselines");
    assert!(baseline(&manifest, &out).status.success());
    let saved = fs::read(out.join("repaired.json")).unwrap();
    let result: Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(result["simfile"], "original.sm");
    assert_eq!(
        result["repair"]["original_sha256"],
        original_doc["source_sha256"]
    );
    assert_eq!(
        result["repair"]["oracle_sha256"],
        fixed_doc["source_sha256"]
    );
    assert_eq!(fs::read_to_string(original_path).unwrap(), original);
    let verified = run(&[Path::new("verify"), &manifest, Path::new("--against"), &out]);
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    work.write("oracle.sm", &(fixed + "\n// unapproved change\n"));
    let rejected = run(&[Path::new("verify"), &manifest, Path::new("--against"), &out]);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("oracle SHA-256 does not match"));
    assert_eq!(fs::read(out.join("repaired.json")).unwrap(), saved);
}

#[test]
fn loader_diagnostics_require_an_exact_manifest_expectation() {
    let work = Workspace::new();
    let text = sm("Valid", "Hard", "1000\n0000\n0100\n0000")
        .replace("#BPMS:0=150.125;", "#BPMS:0=150.125,4=0,8=200;");
    let path = work.write("warning.sm", &text);
    let doc = document(run(&[Path::new("charts"), &path]));
    assert!(doc["diagnostics"].to_string().contains("zero BPM"));
    let manifest_text = "version=2\n[[case]]\nname='warning'\nsimfile='warning.sm'\nall_charts=true\nexpected_chart_count=1\n";
    let manifest = work.write("cases.toml", manifest_text);
    let out = work.0.join("baselines");
    let rejected = baseline(&manifest, &out);
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("unexpected loader diagnostics"));
    assert!(!out.exists());
    work.write(
        "cases.toml",
        &format!(
            "{manifest_text}expected_diagnostics={}\n",
            doc["diagnostics"]
        ),
    );
    let accepted = baseline(&manifest, &out);
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    let verified = run(&[Path::new("verify"), &manifest, Path::new("--against"), &out]);
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
}
