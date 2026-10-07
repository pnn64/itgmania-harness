use crate::{oracle, song_lua_baseline, song_lua_headless, song_lua_oracle};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};

const FIXTURE_SCHEMA_VERSION: u32 = 1;
const DEFAULT_BPM: f32 = 120.0;
const DEFAULT_MAX_BEAT: f32 = 4.0;
/// Challenge, the native `Difficulty` enum code preferred for the context chart.
pub const DEFAULT_DIFFICULTY: i32 = 4;

#[derive(Debug)]
pub struct Report {
    pub simfiles: usize,
    pub evaluated_sessions: usize,
    pub complete: usize,
    pub partial: usize,
    pub failed: usize,
    pub manifest: PathBuf,
}

#[derive(Eq, Hash, PartialEq)]
struct SemanticKey {
    simfile: PathBuf,
    entries: Vec<SemanticEntryKey>,
    title: String,
    difficulty: i32,
    steps_type: String,
    description: String,
    max_beat: u32,
    bpm: u32,
    bpm_segments: Vec<(u32, u32)>,
    beat_step: u32,
    max_events: u32,
    random_seed: u32,
}

#[derive(Eq, Hash, PartialEq)]
struct SemanticEntryKey {
    path: PathBuf,
    layer: &'static str,
    index: u32,
    start_beat: u32,
}

#[derive(Serialize)]
struct Manifest<'a> {
    fixture_schema_version: u32,
    oracle_schema_version: u32,
    harness_version: &'a str,
    itgmania: Itgmania<'a>,
    context: ManifestContext,
    simfiles: Vec<ManifestSimfile>,
}

#[derive(Serialize)]
struct Itgmania<'a> {
    git_revision: &'a str,
    git_dirty: &'a str,
    execution: &'static str,
    launches_executable: bool,
}

#[derive(Serialize)]
struct ManifestContext {
    players: [&'static str; 2],
    preferred_difficulty: &'static str,
    preferred_steps_type: String,
    screen: [u32; 2],
    beat_step: f32,
    max_events: u32,
    random_seed: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    until_beat: Option<f32>,
}

#[derive(Serialize)]
struct ManifestSimfile {
    simfile: String,
    fixture: String,
    title: String,
    lua_entries: usize,
    status: &'static str,
    #[serde(skip_serializing_if = "is_zero")]
    runtime_errors: usize,
    #[serde(skip_serializing_if = "is_zero")]
    dropped_events: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

const fn is_zero(value: &usize) -> bool {
    *value == 0
}

pub fn generate(
    songs_root: &Path,
    output_dir: &Path,
    beat_step: f32,
    max_events: u32,
    difficulty_code: i32,
    steps_type: Option<&str>,
    random_seed: u32,
    until_beat: Option<f32>,
    selected: &[PathBuf],
) -> Result<Report, Error> {
    if until_beat.is_some_and(|beat| !beat.is_finite() || beat < 0.0) {
        return Err(Error::Config(
            "semantic endpoint must be finite and nonnegative".into(),
        ));
    }
    if !beat_step.is_finite() || beat_step <= 0.0 {
        return Err(Error::Config(
            "semantic fixture beat step must be finite and positive".into(),
        ));
    }
    if max_events == 0 {
        return Err(Error::Config(
            "semantic fixture event limit must be positive".into(),
        ));
    }
    if random_seed == 0 || random_seed > i32::MAX as u32 {
        return Err(Error::Config(
            "random seed must be in 1..=2147483647".into(),
        ));
    }
    let songs_root = songs_root
        .canonicalize()
        .map_err(|source| Error::io("open song corpus", songs_root, source))?;
    let mut simfiles = Vec::new();
    if selected.is_empty() {
        song_lua_baseline::collect_simfiles(&songs_root, &mut simfiles)
            .map_err(Error::Discovery)?;
    } else {
        for relative in selected {
            let path = songs_root
                .join(relative)
                .canonicalize()
                .map_err(|source| Error::io("open selected simfile", relative, source))?;
            if !path.starts_with(&songs_root)
                || !path.is_file()
                || !path
                    .extension()
                    .and_then(|v| v.to_str())
                    .is_some_and(|v| v.eq_ignore_ascii_case("sm") || v.eq_ignore_ascii_case("ssc"))
            {
                return Err(Error::Config(format!(
                    "selected simfile must be an .sm or .ssc file inside the corpus: {}",
                    relative.display()
                )));
            }
            simfiles.push(path);
        }
    }
    simfiles.sort();
    simfiles.dedup();
    if simfiles.is_empty() {
        return Err(Error::NoSimfiles(songs_root));
    }

    let mut manifest_entries = Vec::with_capacity(simfiles.len());
    let mut cache = HashMap::new();
    let mut complete = 0;
    let mut partial = 0;
    for simfile in &simfiles {
        let relative_simfile =
            song_lua_baseline::relative_path(&songs_root, simfile).map_err(Error::Discovery)?;
        let relative_fixture = Path::new(&relative_simfile).with_extension(format!(
            "{}.semantic.json",
            simfile
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("simfile")
        ));
        let fixture_path = output_dir.join(&relative_fixture);
        let result = generate_one(
            simfile,
            &relative_simfile,
            beat_step,
            max_events,
            difficulty_code,
            steps_type,
            random_seed,
            until_beat,
            &mut cache,
        );
        let (title, lua_entries, runtime_errors, dropped_events, error) = match result {
            Ok((title, count, document)) => {
                let runtime_errors = document
                    .get("runtime_errors")
                    .and_then(Value::as_array)
                    .map_or(0, Vec::len);
                let dropped_events = document
                    .get("dropped_events")
                    .and_then(Value::as_u64)
                    .and_then(|value| usize::try_from(value).ok())
                    .unwrap_or(0);
                song_lua_baseline::write_compact_json(&fixture_path, &document)
                    .map_err(Error::Discovery)?;
                if runtime_errors == 0 && dropped_events == 0 {
                    complete += 1;
                } else {
                    partial += 1;
                }
                (title, count, runtime_errors, dropped_events, None)
            }
            Err(failure) => (
                failure.title,
                failure.lua_entries,
                0,
                0,
                Some(failure.message),
            ),
        };
        manifest_entries.push(ManifestSimfile {
            simfile: relative_simfile,
            fixture: song_lua_baseline::slash_path(&relative_fixture),
            title,
            lua_entries,
            status: if error.is_some() {
                "error"
            } else if runtime_errors == 0 && dropped_events == 0 {
                "ok"
            } else {
                "partial"
            },
            runtime_errors,
            dropped_events,
            error,
        });
    }

    let manifest_path = output_dir.join("_semantic_manifest.json");
    let manifest = Manifest {
        fixture_schema_version: FIXTURE_SCHEMA_VERSION,
        oracle_schema_version: song_lua_oracle::SCHEMA_VERSION,
        harness_version: env!("CARGO_PKG_VERSION"),
        itgmania: Itgmania {
            git_revision: env!("ITGMANIA_GIT_REVISION"),
            git_dirty: env!("ITGMANIA_GIT_DIRTY"),
            execution: "embedded_bundled_lua",
            launches_executable: false,
        },
        context: ManifestContext {
            players: ["PLAYER_1", "PLAYER_2"],
            preferred_difficulty: difficulty_name(difficulty_code)
                .trim_start_matches("Difficulty_"),
            preferred_steps_type: steps_type.unwrap_or("dance-single").to_owned(),
            screen: [854, 480],
            beat_step,
            max_events,
            random_seed,
            until_beat,
        },
        simfiles: manifest_entries,
    };
    song_lua_baseline::write_json(&manifest_path, &manifest).map_err(Error::Discovery)?;
    Ok(Report {
        simfiles: simfiles.len(),
        evaluated_sessions: cache.len(),
        complete,
        partial,
        failed: simfiles.len() - complete - partial,
        manifest: manifest_path,
    })
}

fn generate_one(
    simfile: &Path,
    relative_simfile: &str,
    beat_step: f32,
    max_events: u32,
    difficulty_code: i32,
    steps_type: Option<&str>,
    random_seed: u32,
    until_beat: Option<f32>,
    cache: &mut HashMap<SemanticKey, Vec<u8>>,
) -> Result<(String, usize, Value), Failure> {
    let discovery = song_lua_oracle::load(simfile).map_err(|error| Failure {
        title: simfile_title(simfile),
        lua_entries: 0,
        message: error.to_string(),
    })?;
    let title = discovery.title.clone();
    let entries = discovery
        .changes
        .iter()
        .filter_map(|change| {
            change
                .lua_entry
                .as_ref()
                .map(|path| song_lua_headless::Entry {
                    path: PathBuf::from(path),
                    layer: layer_name(change.layer),
                    index: change.index,
                    start_beat: change.start_beat.get(),
                })
        })
        .collect::<Vec<_>>();
    let charts = oracle::load(simfile).map_err(|error| Failure {
        title: title.clone(),
        lua_entries: entries.len(),
        message: error.to_string(),
    })?;
    let chart =
        select_chart(&charts.charts, difficulty_code, steps_type).ok_or_else(|| Failure {
            title: title.clone(),
            lua_entries: entries.len(),
            message: format!(
                "simfile has no chart for semantic context: {}, {}",
                steps_type.unwrap_or("default style"),
                difficulty_name(difficulty_code)
            ),
        })?;
    let note_end_beat = chart
        .note_data
        .entries
        .iter()
        .map(|note| note.beat.get() + note.duration_beats.get().max(0.0))
        .filter(|beat| beat.is_finite())
        .fold(DEFAULT_MAX_BEAT, f32::max);
    // Song::ReCalculateStepStatsAndLastSecond retains LASTSECONDHINT beyond
    // the notes. Keep outro callbacks in the capture using the native parser
    // and native timing conversion, without changing explicit micro contexts.
    let specified_last_beat = discovery.specified_last_beat.get();
    let max_beat = if specified_last_beat.is_finite() {
        note_end_beat.max(specified_last_beat)
    } else {
        note_end_beat
    };
    let max_beat = until_beat.map_or(max_beat, |minimum| max_beat.max(minimum));
    let bpm = chart.bpm.actual_max.get();
    let bpm = if bpm.is_finite() && bpm > 0.0 {
        bpm
    } else {
        DEFAULT_BPM
    };
    let mut bpm_segments = chart
        .timing
        .segments
        .iter()
        .filter(|segment| segment.segment_type_code == 0)
        .filter_map(|segment| {
            let value = segment.values.first()?.get();
            let beat = segment.beat.get();
            (beat.is_finite() && value.is_finite() && value > 0.0)
                .then_some(song_lua_headless::BpmSegment { beat, bpm: value })
        })
        .collect::<Vec<_>>();
    bpm_segments.sort_by(|left, right| left.beat.total_cmp(&right.beat));
    if bpm_segments.is_empty() {
        bpm_segments.push(song_lua_headless::BpmSegment { beat: 0.0, bpm });
    }
    let song_dir = simfile.parent().unwrap_or_else(|| Path::new("."));
    let context = song_lua_headless::Context {
        simfile,
        song_dir,
        title: &title,
        difficulty: difficulty_name(chart.difficulty_code),
        steps_type: &chart.steps_type.source,
        description: &chart.description,
        max_beat,
        bpm,
        bpm_segments: &bpm_segments,
        beat_step,
        max_events,
        random_seed,
    };
    let key = SemanticKey {
        simfile: simfile.to_owned(),
        entries: entries
            .iter()
            .map(|entry| SemanticEntryKey {
                path: entry.path.clone(),
                layer: entry.layer,
                index: entry.index,
                start_beat: entry.start_beat.to_bits(),
            })
            .collect(),
        title: title.clone(),
        difficulty: chart.difficulty_code,
        steps_type: chart.steps_type.source.clone(),
        description: chart.description.clone(),
        max_beat: max_beat.to_bits(),
        bpm: bpm.to_bits(),
        bpm_segments: bpm_segments
            .iter()
            .map(|segment| (segment.beat.to_bits(), segment.bpm.to_bits()))
            .collect(),
        beat_step: beat_step.to_bits(),
        max_events,
        random_seed,
    };
    let mut document = if let Some(document) = cache.get(&key) {
        serde_json::from_slice(document).map_err(|error| Failure {
            title: title.clone(),
            lua_entries: entries.len(),
            message: format!("could not restore cached semantic session: {error}"),
        })?
    } else {
        let document =
            song_lua_headless::evaluate(&entries, &context).map_err(|error| Failure {
                title: title.clone(),
                lua_entries: entries.len(),
                message: error.to_string(),
            })?;
        let cached = serde_json::to_vec(&document).map_err(|error| Failure {
            title: title.clone(),
            lua_entries: entries.len(),
            message: format!("could not cache semantic session: {error}"),
        })?;
        cache.insert(key, cached);
        document
    };
    document["simfile"] = Value::String(format!(
        "song:/{}",
        simfile
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("unknown")
    ));
    document["source_simfile"] = Value::String(relative_simfile.to_owned());
    document["fixture_context"] = serde_json::json!({
        "chart_description": chart.description,
        "difficulty_code": chart.difficulty_code,
        "steps_type_code": chart.steps_type.code,
        "max_beat": max_beat,
        "note_end_beat": note_end_beat,
        "specified_last_second": discovery.specified_last_second,
        "specified_last_beat": discovery.specified_last_beat,
        "bpm": bpm,
        "bpm_segments": bpm_segments
            .iter()
            .map(|segment| [segment.beat, segment.bpm])
            .collect::<Vec<_>>(),
        "update_fps": 60,
        "beat_step": beat_step,
    });
    crate::song_lua_semantics::compact_fixture(&mut document).map_err(|error| Failure {
        title: title.clone(),
        lua_entries: entries.len(),
        message: error.to_string(),
    })?;
    Ok((title, entries.len(), document))
}

/// The native `Difficulty` enum code for a difficulty name such as `Edit`.
pub fn difficulty_code(name: &str) -> Option<i32> {
    (0..=5).find(|&code| {
        difficulty_name(code)
            .trim_start_matches("Difficulty_")
            .eq_ignore_ascii_case(name)
    })
}

fn select_chart<'a>(
    charts: &'a [oracle::Chart],
    difficulty_code: i32,
    steps_type: Option<&str>,
) -> Option<&'a oracle::Chart> {
    if let Some(steps_type) = steps_type {
        return charts.iter().find(|chart| {
            chart.steps_type.source == steps_type && chart.difficulty_code == difficulty_code
        });
    }
    charts
        .iter()
        .find(|chart| {
            chart.steps_type.source == "dance-single" && chart.difficulty_code == difficulty_code
        })
        .or_else(|| {
            charts
                .iter()
                .find(|chart| chart.steps_type.source == "dance-single")
        })
        .or_else(|| charts.first())
}

#[cfg(all(test, itgmania_oracle))]
mod native_tests {
    use super::*;

    #[test]
    fn explicit_style_selects_native_double_chart() {
        let song = oracle::load(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/song-lua-headless/song-steps.sm"),
        )
        .expect("load native chart list");
        let chart =
            select_chart(&song.charts, 3, Some("dance-double")).expect("native double Hard chart");
        assert_eq!(chart.steps_type.source, "dance-double");
        assert_eq!(chart.difficulty_code, 3);
        assert!(select_chart(&song.charts, 4, Some("dance-double")).is_none());
        assert!(select_chart(&song.charts, 4, Some("missing-style")).is_none());
    }
}

const fn difficulty_name(code: i32) -> &'static str {
    match code {
        0 => "Difficulty_Beginner",
        1 => "Difficulty_Easy",
        2 => "Difficulty_Medium",
        3 => "Difficulty_Hard",
        4 => "Difficulty_Challenge",
        5 => "Difficulty_Edit",
        _ => "Difficulty_Invalid",
    }
}

const fn layer_name(layer: song_lua_oracle::Layer) -> &'static str {
    match layer {
        song_lua_oracle::Layer::Background1 => "background1",
        song_lua_oracle::Layer::Background2 => "background2",
        song_lua_oracle::Layer::Foreground => "foreground",
    }
}

fn simfile_title(path: &Path) -> String {
    path.file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown")
        .to_owned()
}

struct Failure {
    title: String,
    lua_entries: usize,
    message: String,
}

#[derive(Debug)]
pub enum Error {
    Config(String),
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    NoSimfiles(PathBuf),
    Discovery(song_lua_baseline::Error),
}

impl Error {
    fn io(action: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_owned(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config(message) => formatter.write_str(message),
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "could not {action} {}: {source}", path.display()),
            Self::NoSimfiles(path) => write!(
                formatter,
                "no .sm, .sma, .ssc, or .ats files found under {}",
                path.display()
            ),
            Self::Discovery(error) => error.fmt(formatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_fixture_suffix_preserves_simfile_extension() {
        assert_eq!(
            Path::new("Song/file.ssc").with_extension("ssc.semantic.json"),
            PathBuf::from("Song/file.ssc.semantic.json")
        );
    }
}
