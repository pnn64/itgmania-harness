#![cfg(any(target_os = "linux", target_os = "windows"))]

use serde_json::Value;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const BIN: &str = env!("CARGO_BIN_EXE_itgmania-harness-rs");

mod support;

fn itgmania_root() -> PathBuf {
    PathBuf::from(env!("ITGMANIA_BUILD_ROOT"))
}

#[test]
fn captures_selected_empty_charts_and_isolates_file_writes() {
    let directory = temp_dir("song-host");
    let corpus = directory.join("corpus");
    std::fs::create_dir_all(&corpus).unwrap();
    let chart = "#TITLE:Host;\n#BPMS:0=60;\n#NOTES:dance-single::Challenge:1:0,0,0,0,0:\n1000\n0100\n0010\n0001\n;\n";
    std::fs::write(corpus.join("empty.sm"), chart).unwrap();
    let traces = directory.join("empty-traces");
    let output = run(&[
        "song-lua-semantic-baseline",
        corpus.to_str().unwrap(),
        "--simfile",
        "empty.sm",
        "--simfile",
        "empty.sm",
        "--out",
        traces.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(traces.join("_semantic_manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["simfiles"].as_array().unwrap().len(), 1);
    assert_eq!(manifest["simfiles"][0]["status"], "ok");
    assert_eq!(manifest["simfiles"][0]["lua_entries"], 0);
    let archives = directory.join("archives");
    let output = run(&[
        "song-lua-archive",
        corpus.to_str().unwrap(),
        "--traces",
        traces.to_str().unwrap(),
        "--out",
        archives.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let output = run(&[
        "song-lua-semantic-baseline",
        corpus.to_str().unwrap(),
        "--simfile",
        "../outside.sm",
        "--out",
        traces.to_str().unwrap(),
    ]);
    assert!(!output.status.success());
    std::fs::write(
        corpus.join("write.sm"),
        format!("#FGCHANGES:0=write.lua;\n{chart}"),
    )
    .unwrap();
    std::fs::write(
        corpus.join("write.lua"),
        r#"
local directory = GAMESTATE:GetCurrentSong():GetSongDir()
local file = RageFileUtil.CreateRageFile()
assert(file:Open(directory .. "generated.lua", 2))
assert(file:Write("return 'captured'\n") > 0)
file:Close()
assert(file:Open(directory .. "generated.lua", 1))
assert(file:Read() == "return 'captured'\n")
file:destroy()
assert(assert(loadfile(directory .. "generated.lua"))() == "captured")
local outside = RageFileUtil.CreateRageFile()
assert(not outside:Open(directory .. "../escape.txt", 2))
outside:destroy()
return Def.Actor{}
"#,
    )
    .unwrap();
    let traces = directory.join("write-traces");
    let output = Command::new(BIN)
        .args([
            "song-lua-semantic-baseline",
            corpus.to_str().unwrap(),
            "--simfile",
            "write.sm",
            "--out",
            traces.to_str().unwrap(),
        ])
        .env("ITGMANIA_SONG_LUA_WRITE_ROOT", &corpus)
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(traces.join("_semantic_manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest["simfiles"][0]["status"], "ok", "{manifest}");
    let trace: Value =
        serde_json::from_slice(&std::fs::read(traces.join("write.sm.semantic.json")).unwrap())
            .unwrap();
    assert_eq!(trace["capabilities"]["isolated_file_writes"], true);
    assert!(!trace["file_writes"].as_array().unwrap().is_empty());
    assert!(
        !trace["loaded_lua_files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p == "song:/generated.lua")
    );
    assert!(!directory.join("escape.txt").exists());
    assert_eq!(
        std::fs::read_to_string(corpus.join("empty.sm")).unwrap(),
        chart
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn doctor_uses_vendored_sources_from_another_working_directory() {
    let output = Command::new(BIN)
        .arg("doctor")
        .env_remove("ITGMANIA_ROOT")
        .current_dir(std::env::temp_dir())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("vendor\\itgmania") || text.contains("vendor/itgmania"));
    assert!(text.contains("status: ITGmania source tree is usable"));
}

#[test]
fn reads_native_music_duration_and_reports_corrupt_audio() {
    let directory = temp_dir("music-duration");
    std::fs::create_dir_all(&directory).unwrap();
    let mut wave = b"RIFF".to_vec();
    wave.extend(16036u32.to_le_bytes());
    wave.extend(b"WAVEfmt ");
    wave.extend(16u32.to_le_bytes());
    wave.extend(1u16.to_le_bytes()); // PCM
    wave.extend(1u16.to_le_bytes()); // mono
    wave.extend(8000u32.to_le_bytes());
    wave.extend(16000u32.to_le_bytes());
    wave.extend(2u16.to_le_bytes());
    wave.extend(16u16.to_le_bytes());
    wave.extend(b"data");
    wave.extend(16000u32.to_le_bytes());
    wave.resize(16044, 0); // exactly one second of PCM silence
    std::fs::write(directory.join("music.wav"), wave).unwrap();
    std::fs::write(directory.join("duration.lua"),
        "assert(GAMESTATE:GetCurrentSong():MusicLengthSeconds() == 1); return Def.Actor{}").unwrap();
    std::fs::write(directory.join("music.sm"),
        "#TITLE:Music;\n#MUSIC:music.wav;\n#BPMS:0=60;\n#FGCHANGES:0=duration.lua;\n#NOTES:dance-single::Challenge:1:0,0,0,0,0:\n1000\n0100\n0010\n0001\n;\n").unwrap();
    for (label, expected) in [("valid", "ok"), ("corrupt", "error")] {
        if label == "corrupt" {
            std::fs::write(directory.join("music.wav"), b"invalid wave").unwrap();
        }
        let traces = directory.join(label);
        let output = run(&["song-lua-semantic-baseline", directory.to_str().unwrap(),
            "--simfile", "music.sm", "--out", traces.to_str().unwrap()]);
        assert!(output.status.success(), "{}", stderr(&output));
        let manifest: Value = serde_json::from_slice(&std::fs::read(traces.join("_semantic_manifest.json")).unwrap()).unwrap();
        assert_eq!(manifest["simfiles"][0]["status"], expected, "{manifest}");
        if label == "valid" {
            let trace: Value = serde_json::from_slice(&std::fs::read(traces.join("music.sm.semantic.json")).unwrap()).unwrap();
            assert!(trace["file_reads"].as_array().unwrap().iter()
                .any(|read| read["path"] == "song:/music.wav" && read["exists"] == true));
        }
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn doctor_explains_how_to_initialize_missing_sources() {
    let directory = temp_dir("missing-sources");
    std::fs::create_dir_all(&directory).unwrap();
    let output = run(&["doctor", "--itgmania-root", directory.to_str().unwrap()]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("git submodule update --init"));
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn exports_raw_itgmania_values() {
    let fixture = fixture();
    let output = run(&["charts", fixture.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));

    let text = String::from_utf8(output.stdout).unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let chart = &document["charts"][0];

    assert_eq!(document["schema_version"], 5);
    assert_eq!(chart["title"], "Raw Value Fixture");
    assert_eq!(chart["steps_type"]["source"], "dance-single");
    assert_eq!(chart["difficulty_code"], 4);
    assert_eq!(chart["meter"], 9);
    assert_eq!(chart["note_data"]["tracks"], 4);
    assert_eq!(
        chart["player_stats"][0]["radar"].as_array().unwrap().len(),
        14
    );
    assert_eq!(
        chart["player_stats"][0]["tech_counts"]
            .as_array()
            .unwrap()
            .len(),
        10
    );
    let note_types = chart["note_data"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|note| note["note_type_code"].as_i64().unwrap())
        .collect::<Vec<_>>();
    for expected in [1, 2, 4, 5, 8] {
        assert!(note_types.contains(&expected));
    }
    assert!(text.contains("0.12345679"));
    assert!(!text.contains("0.123457"));
}

#[test]
fn selects_by_exact_native_identity() {
    let fixture = fixture();
    let output = run(&[
        "chart",
        fixture.to_str().unwrap(),
        "--steps-type",
        "dance-single",
        "--difficulty-code",
        "4",
        "--description",
        "Exact selector",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["charts"].as_array().unwrap().len(), 1);
    assert_eq!(document["charts"][0]["description"], "Exact selector");
}

#[test]
fn rejects_ambiguous_semantic_selector() {
    let fixture = fixture();
    let output = run(&[
        "chart",
        fixture.to_str().unwrap(),
        "--steps-type",
        "dance-single",
    ]);
    assert!(!output.status.success());
    assert!(stderr(&output).contains("matched multiple charts at indices 0, 1"));
}

#[test]
fn loads_sm_through_itgmania() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/basic.sm");
    let output = run(&[
        "chart",
        fixture.to_str().unwrap(),
        "--steps-type",
        "dance-single",
        "--difficulty-code",
        "3",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    let chart = &document["charts"][0];
    assert_eq!(chart["title"], "SM Loader Fixture");
    assert_eq!(chart["description"], "SM exact selector");
    assert_eq!(chart["meter"], 7);
    assert_eq!(chart["bpm"]["actual_min"], 150.125);
    assert_eq!(chart["note_data"]["entries"].as_array().unwrap().len(), 4);
}

#[test]
fn exports_itgmania_font_geometry() {
    let font = support::theme_root().join("Fonts/Miso/_miso light.ini");
    let output = run(&["font", font.to_str().unwrap(), "--text", "Ag 1"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["text"], "Ag 1");
    assert_eq!(document["height"], 15);
    assert_eq!(document["line_spacing"], 24);
    assert_eq!(document["line_width"], 29);
    assert_eq!(document["observed_pages"][0]["source_size"]["width"], 360);
    assert_eq!(document["observed_pages"][0]["frames"]["width"], 15);
    assert_eq!(document["glyphs"][0]["codepoint"], 65);
    assert_eq!(document["glyphs"][0]["frame"], 33);
    assert_eq!(document["glyphs"][0]["advance"], 9);
    assert_eq!(document["glyphs"][1]["pen_x"], 9);
    assert_eq!(document["diagnostics"].as_array().unwrap().len(), 0);
    assert_eq!(
        PathBuf::from(document["fallback_fonts"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        itgmania_root()
            .join("Themes/_fallback/Fonts")
            .canonicalize()
            .unwrap()
    );

    let redirect = support::theme_root().join("Fonts/Common Normal.redir");
    let redirected = run(&["font", redirect.to_str().unwrap(), "--text", "A"]);
    assert!(redirected.status.success(), "{}", stderr(&redirected));
    let redirected: Value = serde_json::from_slice(&redirected.stdout).unwrap();
    assert!(
        redirected["font"]
            .as_str()
            .unwrap()
            .ends_with("Fonts/Miso/_miso light.ini")
    );
}

#[test]
fn exports_itgmania_noteskin_semantics() {
    let root = itgmania_root().join("NoteSkins");
    let output = run(&["noteskin", root.to_str().unwrap(), "dance", "default"]);
    assert!(output.status.success(), "{}", stderr(&output));

    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["game"], "dance");
    assert_eq!(document["skin"], "default");
    assert_eq!(document["inventory"].as_array().unwrap().len(), 12);
    assert_eq!(document["diagnostics"].as_array().unwrap().len(), 0);
    assert!(
        document["metrics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|metric| {
                metric["section"] == "NoteDisplay"
                    && metric["key"] == "TapNoteAnimationLength"
                    && metric["float"] == 1.0
            })
    );
    assert!(document["paths"].as_array().unwrap().iter().any(|path| {
        path["button"] == "Down"
            && path["element"] == "Tap Note"
            && path["path"].as_str().is_some_and(|value| {
                value
                    .replace('\\', "/")
                    .ends_with("NoteSkins/dance/default/Down Tap Note.lua")
            })
    }));
}

#[test]
fn analyzes_song_lua_trace_offline() {
    let directory = temp_dir("raw-song-lua-trace");
    std::fs::create_dir_all(&directory).unwrap();
    let trace = directory.join("trace.json");
    let children = (1..=8)
        .map(|index| {
            serde_json::json!({
                "layer_index": index,
                "definition_id": format!("def-{index}")
            })
        })
        .collect::<Vec<_>>();
    let mut definitions = vec![serde_json::json!({
        "id": "root", "runtime_actors": ["root"], "children": children
    })];
    definitions.extend((1..=8).map(|index| {
        serde_json::json!({
            "id": format!("def-{index}"),
            "runtime_actors": [format!("def-{index}")],
            "children": []
        })
    }));
    let raw = serde_json::json!({
        "actor_definitions": definitions,
        "events": [
            {
                "seq": 1, "kind": "call", "actor": "def-1",
                "operation": "Actor.decelerate", "args": [0.6],
                "command": "RotateEmMessageCommand", "beat": 111.75
            },
            {
                "seq": 2, "kind": "call", "actor": "def-1",
                "operation": "Actor.addrotationz", "args": [360],
                "command": "RotateEmMessageCommand", "beat": 111.75
            }
        ]
    });
    std::fs::write(&trace, serde_json::to_vec(&raw).unwrap()).unwrap();
    let output = run(&["song-lua-analyze", trace.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));

    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["capabilities"]["tween_segments"], true);
    assert_eq!(document["capabilities"]["stable_draw_order"], true);
    assert_eq!(
        document["draw_orders"][0]["final_children"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert!(
        document["tween_segments"]
            .as_array()
            .unwrap()
            .iter()
            .any(|segment| {
                segment["command"] == "RotateEmMessageCommand"
                    && segment["easing"] == "decelerate"
                    && segment["operations"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|operation| operation["operation"] == "Actor.addrotationz")
            })
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn writes_portable_noteskin_baselines() {
    let root = itgmania_root().join("NoteSkins");
    let output_dir = temp_dir("noteskin-baselines");
    let output = run(&[
        "noteskin-baseline",
        root.to_str().unwrap(),
        "--out",
        output_dir.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let manifest: Value =
        serde_json::from_slice(&std::fs::read(output_dir.join("_manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["fixture_schema_version"], 1);
    assert_eq!(manifest["fixtures"].as_array().unwrap().len(), 28);
    assert_eq!(manifest["source_files"].as_array().unwrap().len(), 940);
    assert!(
        manifest["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| {
                !entry["fixture"].as_str().unwrap().contains('\\')
                    && output_dir
                        .join(entry["fixture"].as_str().unwrap())
                        .is_file()
            })
    );

    std::fs::remove_dir_all(output_dir).unwrap();
}

#[test]
#[ignore = "requires the external Delightful Day song corpus"]
fn exports_itgmania_song_lua_layers() {
    let simfile = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../lua-songs/Delightful Day/Delightful Day.ssc");
    let output = run(&["song-lua", simfile.to_str().unwrap()]);
    assert!(output.status.success(), "{}", stderr(&output));

    let document: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["schema_version"], 2);
    assert_eq!(document["title"], "Delightful Day");
    let changes = document["changes"].as_array().unwrap();
    assert_eq!(changes[0]["layer"], "background1");
    assert_eq!(changes[0]["file1"], "Delightful Day.mp4");
    assert!(changes.iter().any(|change| {
        change["layer"] == "foreground"
            && change["start_beat"] == 0.0
            && change["lua_entry"]
                .as_str()
                .is_some_and(|path| path.replace('\\', "/").ends_with("lua/default.lua"))
    }));
}

#[test]
fn semantic_capture_keeps_last_second_hint() {
    let directory = temp_dir("song-lua-outro");
    std::fs::create_dir_all(&directory).unwrap();
    let simfile = directory.join("outro.ssc");
    std::fs::write(&simfile, "#TITLE:Outro;\n#BPMS:0=120;\n#LASTSECONDHINT:2.5;\n#FGCHANGES:0=mods.lua=1=0=0=1=====;\n#NOTEDATA:;\n#STEPSTYPE:dance-single;\n#DIFFICULTY:Challenge;\n#METER:1;\n#NOTES:1000\n0000\n0000\n0000\n;\n").unwrap();
    std::fs::write(
        directory.join("mods.lua"),
        r#"return Def.Quad{
        OnCommand=function(self)
            self:SetUpdateFunction(function(actor)
                if GAMESTATE:GetSongBeat()>6 then actor:diffusealpha(0.25)
                elseif GAMESTATE:GetSongBeat()>4 then actor:diffusealpha(0.5) end
            end)
        end,
    }"#,
    )
    .unwrap();
    let metadata = run(&["song-lua", simfile.to_str().unwrap()]);
    assert!(metadata.status.success(), "{}", stderr(&metadata));
    let metadata: Value = serde_json::from_slice(&metadata.stdout).unwrap();
    assert_eq!(metadata["specified_last_second"], 2.5);
    assert_eq!(metadata["specified_last_beat"], 5.0);
    let output_dir = directory.join("captured");
    let output = run(&[
        "song-lua-semantic-baseline",
        directory.to_str().unwrap(),
        "--out",
        output_dir.to_str().unwrap(),
        "--beat-step",
        "0.125",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let trace: Value =
        serde_json::from_slice(&std::fs::read(output_dir.join("outro.ssc.semantic.json")).unwrap())
            .unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["fixture_context"]["note_end_beat"], 4.0);
    assert_eq!(trace["trace_until_beat"], 5.0);
    assert_eq!(trace["end_position"]["seconds"], 2.5);
    assert!(
        trace["projected_vertex_tracks"][0]["samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|sample| sample[0].as_f64().is_some_and(|beat| beat > 4.0) && sample[3] == 0.5),
        "the post-note outro must run through the native LASTSECONDHINT"
    );
    let extended = directory.join("extended");
    let output = run(&[
        "song-lua-semantic-baseline",
        directory.to_str().unwrap(),
        "--out",
        extended.to_str().unwrap(),
        "--until-beat",
        "7",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let trace: Value =
        serde_json::from_slice(&std::fs::read(extended.join("outro.ssc.semantic.json")).unwrap())
            .unwrap();
    assert_eq!(trace["trace_until_beat"], 7.0);
    assert!(
        trace["projected_vertex_tracks"][0]["samples"]
            .as_array()
            .unwrap()
            .iter()
            .any(|sample| sample[0].as_f64().is_some_and(|beat| beat > 6.0) && sample[3] == 0.25),
        "an explicit minimum endpoint must retain later callbacks too"
    );
    for invalid in ["-1", "NaN", "inf"] {
        let output = run(&[
            "song-lua-semantic-baseline",
            directory.to_str().unwrap(),
            "--out",
            extended.to_str().unwrap(),
            "--until-beat",
            invalid,
        ]);
        assert!(
            !output.status.success(),
            "invalid endpoint {invalid} must fail"
        );
    }
}

#[test]
#[ignore = "requires the external lua-songs corpus"]
fn writes_portable_song_lua_baselines() {
    let songs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../lua-songs");
    let output_dir = temp_dir("song-lua-baselines");
    let output = run(&[
        "song-lua-baseline",
        songs.to_str().unwrap(),
        "--out",
        output_dir.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let manifest: Value =
        serde_json::from_slice(&std::fs::read(output_dir.join("_manifest.json")).unwrap()).unwrap();
    let simfiles = manifest["simfiles"].as_array().unwrap();
    assert_eq!(manifest["fixture_schema_version"], 1);
    // The corpus grows independently of the harness. Check actual coverage,
    // rather than freezing the number of songs at an older corpus revision.
    let mut pending = vec![songs.clone()];
    let mut source_paths = std::collections::BTreeSet::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| {
                matches!(
                    ext.to_string_lossy().to_ascii_lowercase().as_str(),
                    "sm" | "sma" | "ssc" | "ats"
                )
            }) {
                source_paths.insert(
                    path.strip_prefix(&songs)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let captured_paths = simfiles
        .iter()
        .map(|entry| entry["simfile"].as_str().unwrap().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(captured_paths, source_paths);
    assert_eq!(captured_paths.len(), simfiles.len());
    assert!(simfiles.iter().all(|entry| {
        let fixture = output_dir.join(entry["fixture"].as_str().unwrap());
        let doc: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
        let count = doc["changes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|change| change["lua_entry"].is_string())
            .count();
        !entry["fixture"].as_str().unwrap().contains('\\')
            && entry["lua_entries"].as_u64().unwrap() == count as u64
    }));

    std::fs::remove_dir_all(output_dir).unwrap();
}

#[test]
#[ignore = "requires the external DeadSync font corpus"]
fn writes_portable_font_baselines() {
    let font_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../deadsync/assets/fonts");
    let output_dir = temp_dir("font-baselines");
    let output = run(&[
        "font-baseline",
        font_root.to_str().unwrap(),
        "--out",
        output_dir.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let manifest: Value =
        serde_json::from_slice(&std::fs::read(output_dir.join("_manifest.json")).unwrap()).unwrap();
    let fonts = manifest["fonts"].as_array().unwrap();
    assert_eq!(manifest["fixture_schema_version"], 1);
    assert_eq!(fonts.len(), 22);
    assert!(fonts.iter().all(|font| {
        !font["font"].as_str().unwrap().contains('\\')
            && output_dir.join(font["fixture"].as_str().unwrap()).is_file()
    }));

    let emoji: Value =
        serde_json::from_slice(&std::fs::read(output_dir.join("emoji/_emoji 16px.json")).unwrap())
            .unwrap();
    assert!(
        emoji["glyphs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|glyph| glyph["codepoint"].as_u64().unwrap() > 0xFFFF)
    );

    std::fs::remove_dir_all(output_dir).unwrap();
}

#[test]
fn writes_manifest_baselines_with_provenance() {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let manifest = fixture_dir.join("baselines.toml");
    let output_dir = temp_dir("chart-baselines");
    let output = run(&[
        "baseline",
        manifest.to_str().unwrap(),
        "--out",
        output_dir.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));

    let raw: Value = serde_json::from_slice(
        &std::fs::read(output_dir.join("raw-values-challenge.json")).unwrap(),
    )
    .unwrap();
    let sm: Value =
        serde_json::from_slice(&std::fs::read(output_dir.join("sm-hard.json")).unwrap()).unwrap();
    let provenance: Value =
        serde_json::from_slice(&std::fs::read(output_dir.join("_manifest.json")).unwrap()).unwrap();
    assert_eq!(raw["simfile"], "raw-values.ssc");
    assert_eq!(raw["charts"][0]["difficulty_code"], 4);
    assert_eq!(sm["simfile"], "basic.sm");
    assert_eq!(provenance["oracle_schema_version"], 5);
    assert_eq!(provenance["cases"].as_array().unwrap().len(), 2);
    let revision = Command::new("git")
        .arg("-C")
        .arg(itgmania_root())
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(revision.status.success());
    assert_eq!(
        provenance["itgmania"]["git_revision"].as_str().unwrap(),
        String::from_utf8_lossy(&revision.stdout).trim()
    );

    let verify = run(&[
        "verify",
        manifest.to_str().unwrap(),
        "--against",
        output_dir.to_str().unwrap(),
    ]);
    assert!(verify.status.success(), "{}", stderr(&verify));
    assert!(String::from_utf8_lossy(&verify.stdout).contains("verified 2 baselines"));

    let raw_path = output_dir.join("raw-values-challenge.json");
    let mut changed = raw;
    changed["charts"][0]["meter"] = Value::from(99);
    std::fs::write(&raw_path, serde_json::to_vec_pretty(&changed).unwrap()).unwrap();
    let mismatch = run(&[
        "verify",
        manifest.to_str().unwrap(),
        "--against",
        output_dir.to_str().unwrap(),
    ]);
    assert!(!mismatch.status.success());
    let error = stderr(&mismatch);
    assert!(error.contains("baseline `raw-values-challenge`"));
    assert!(error.contains("$.charts[0].meter: expected 99, actual 9"));
    let untouched: Value = serde_json::from_slice(&std::fs::read(raw_path).unwrap()).unwrap();
    assert_eq!(untouched["charts"][0]["meter"], 99);

    std::fs::remove_dir_all(output_dir).unwrap();
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw-values.ssc")
}

fn temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "itgmania-harness-rs-{label}-{}-{unique}",
        std::process::id()
    ))
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(BIN).args(args).output().unwrap()
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
