use serde_json::Value;
use sha2::Digest;
use std::fmt;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const THEME_NAME: &str = "itgmania-harness";
const RESULT_PATH: &str = "Save/itgmania-harness/song-lua-trace.json";
const INIT_LUA: &str = include_str!("../probe_theme/ScreenSongLuaHarness overlay.lua");
const GAMEPLAY_LUA: &str = include_str!("../probe_theme/ScreenGameplay overlay.lua");
const SYSTEM_LUA: &str = include_str!("../probe_theme/ScreenSystemLayer overlay.lua");
const TRACE_JSON_LUA: &str = include_str!("../probe_theme/TraceJson.lua");
const TRACE_LUA: &str = include_str!("../probe_theme/SongLuaTrace.lua");
const NOTEFIELD_LUA: &str = include_str!("../probe_theme/NotefieldTrace.lua");
const RENDER_TARGET_PASS_PROTOCOL: &[u8] = b"ITGMANIA_HARNESS_RENDER_TARGET_PASS_PROTOCOL";
pub const DEFAULT_MAX_EVENTS: u32 = 250_000;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ScheduledInput {
    pub scene: u32,
    pub seconds: f64,
    pub player: u8,
    pub button: String,
    pub key: String,
    pub down: bool,
}

#[derive(Debug)]
pub struct Options {
    pub simfile: PathBuf,
    pub itgmania_bin: PathBuf,
    pub until_beat: f64,
    pub game: String,
    pub style: String,
    pub difficulty: Option<String>,
    pub chart_index: Option<u32>,
    pub fallback_theme: String,
    pub timeout: Duration,
    pub keep_run_dir: bool,
    pub headless: bool,
    pub until_gameplay_end: bool,
    pub autoplay: bool,
    pub max_events: u32,
    pub random_seed: u32,
    pub inputs: Vec<ScheduledInput>,
    pub drawable_samples: bool,
    pub capture_fps: Option<u32>,
    pub fixed_update_fps: Option<u32>,
    pub stop_lua_gc: bool,
    pub notefield_samples: bool,
    pub scene_draws: bool,
    pub render_target_passes: bool,
    pub drawable_from_beat: f64,
    pub player_options: Option<String>,
    pub fast_note_rendering: Option<bool>,
}

pub fn capture(options: &Options) -> Result<Value, Error> {
    validate_options(options)?;
    let simfile = canonicalize("open simfile", &options.simfile)?;
    let binary = canonicalize("open ITGmania executable", &options.itgmania_bin)?;
    if !simfile.is_file() {
        return Err(Error::new(format!(
            "simfile path {} is not a file",
            simfile.display()
        )));
    }
    if !binary.is_file() {
        return Err(Error::new(format!(
            "ITGmania executable path {} is not a file",
            binary.display()
        )));
    }
    let install_root = install_root(&binary)?;
    let run_dir = create_run_dir()?;
    let result = capture_in(options, &simfile, &binary, &install_root, &run_dir)
        .map_err(|error| error.with_run_dir(&run_dir));
    if result.is_ok() && !options.keep_run_dir {
        let _ = fs::remove_dir_all(&run_dir);
    }
    result
}

fn validate_options(options: &Options) -> Result<(), Error> {
    if options.game.trim().is_empty() {
        return Err(Error::new("game must not be empty"));
    }
    if options.chart_index == Some(0) {
        return Err(Error::new("chart index must be greater than zero"));
    }
    if !options.drawable_from_beat.is_finite() || options.drawable_from_beat < 0.0
        || !options.until_gameplay_end && options.drawable_from_beat > options.until_beat
    { return Err(Error::new("drawable start beat must be within the capture")); }
    if options.notefield_samples && !options.drawable_samples {
        return Err(Error::new("notefield samples require drawable capture"));
    }
    if options.scene_draws && !options.notefield_samples {
        return Err(Error::new("scene draws require the notefield geometry capture profile"));
    }
    if options.render_target_passes && (!options.drawable_samples || options.notefield_samples) {
        return Err(Error::new("render target passes require the semantic --drawable-samples profile"));
    }
    if options.notefield_samples && (!options.headless || !options.inputs.is_empty()) {
        return Err(Error::new("notefield geometry requires headless capture without scripted inputs"));
    }
    let mut previous = (0, 0.0);
    for event in &options.inputs {
        if event.scene == 0 || !event.seconds.is_finite() || event.seconds < 0.0
            || !(1..=2).contains(&event.player) || event.button.is_empty() || event.key.is_empty()
            || (event.scene, event.seconds) < previous
        { return Err(Error::new("input script requires ordered scene/music-seconds values, player 1 or 2, and native button/key names")); }
        previous = (event.scene, event.seconds);
    }
    if options.random_seed == 0 || options.random_seed > i32::MAX as u32 {
        return Err(Error::new("random seed must be in 1..=2147483647"));
    }
    if options.capture_fps.is_some_and(|fps| !(1..=1000).contains(&fps)) {
        return Err(Error::new("capture FPS must be in 1..=1000"));
    }
    if options.fixed_update_fps.is_some_and(|fps| !(1..=1000).contains(&fps)) {
        return Err(Error::new("fixed update FPS must be in 1..=1000"));
    }
    if options.max_events == 0 {
        return Err(Error::new("runtime event limit must be greater than zero"));
    }
    if !options.until_beat.is_finite() || options.until_beat < 0.0 {
        return Err(Error::new("trace end beat must be finite and non-negative"));
    }
    if options.timeout.is_zero() {
        return Err(Error::new("runtime timeout must be greater than zero"));
    }
    if options.style.trim().is_empty() {
        return Err(Error::new("style must not be empty"));
    }
    if options.fallback_theme.trim().is_empty() {
        return Err(Error::new("fallback theme must not be empty"));
    }
    Ok(())
}

fn capture_in(
    options: &Options,
    simfile: &Path,
    binary: &Path,
    install_root: &Path,
    run_dir: &Path,
) -> Result<Value, Error> {
    prepare_install(options, simfile, binary, install_root, run_dir)?;
    let staged_binary = run_dir.join(binary.strip_prefix(install_root).map_err(|_| {
        Error::new(format!(
            "ITGmania executable {} is outside install root {}",
            binary.display(),
            install_root.display()
        ))
    })?);
    let result_path = run_dir.join(RESULT_PATH);
    let headless_graphics = options.headless && options.drawable_samples;
    if headless_graphics {
        let binary = fs::read(&staged_binary).map_err(|error| Error::new(error.to_string()))?;
        require_headless_graphics_adapter(&binary)?;
    }
    if options.render_target_passes {
        let binary = fs::read(&staged_binary).map_err(|error| Error::new(error.to_string()))?;
        if !binary.windows(RENDER_TARGET_PASS_PROTOCOL.len()).any(|window| window == RENDER_TARGET_PASS_PROTOCOL) {
            return Err(Error::new("render target pass capture requires the native observer; rebuild the trace executable"));
        }
    }
    if options.notefield_samples {
        let bytes = fs::read(&staged_binary).map_err(|error| Error::new(error.to_string()))?;
        if !bytes.windows(b"ITGMANIA_HARNESS_MODEL_GEOMETRY_PROTOCOL_V2".len())
            .any(|window| window == b"ITGMANIA_HARNESS_MODEL_GEOMETRY_PROTOCOL_V2")
        { return Err(Error::new("notefield capture requires the native model geometry adapter; rebuild the trace executable")); }
        if !bytes.windows(b"ITGMANIA_HARNESS_NATIVE_SOURCE".len())
            .any(|window| window == b"ITGMANIA_HARNESS_NATIVE_SOURCE")
        { return Err(Error::new("notefield capture requires native source provenance; rebuild the trace executable")); }
        if !bytes.windows(b"ITGMANIA_HARNESS_RECEPTOR_GEOMETRY_PROTOCOL_V1".len())
            .any(|window| window == b"ITGMANIA_HARNESS_RECEPTOR_GEOMETRY_PROTOCOL_V1")
        { return Err(Error::new("notefield capture requires the native receptor observer; rebuild the trace executable")); }
        if !bytes.windows(b"ITGMANIA_HARNESS_MODEL_RENDER_STATE_PROTOCOL_V1".len())
            .any(|window| window == b"ITGMANIA_HARNESS_MODEL_RENDER_STATE_PROTOCOL_V1")
        { return Err(Error::new("notefield capture requires the native Model render-state observer; rebuild the trace executable")); }
    }
    let mut command = Command::new(&staged_binary);
    // Unix crash reporting writes $HOME/crashinfo.txt. Keep it with this run's
    // diagnostics, including when systemd does not provide a HOME environment.
    command.env("HOME", run_dir);
    if options.headless { command.env("ITGMANIA_HARNESS_HEADLESS", "1"); }
    if headless_graphics { command.env("ITGMANIA_HARNESS_HEADLESS_GRAPHICS", "1"); }
    let mut child = command.args([format!("--game={}", options.game), format!("--theme={THEME_NAME}")])
        .current_dir(staged_binary.parent().unwrap_or(run_dir))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| {
            Error::new(format!(
                "could not launch staged ITGmania executable {}: {source}",
                staged_binary.display()
            ))
        })?;

    // RageFile WRITE uses a temporary file and publishes it on Close.
    let started = Instant::now();
    let result = (|| loop {
        if result_path.is_file() {
            // Close atomically publishes the complete trace. Stop the engine before
            // parsing it so evaluation cannot compete with analysis for CPU/RAM.
            let _ = child.kill();
            let _ = child.wait();
            let file = fs::File::open(&result_path).map_err(|source| {
                Error::new(format!(
                    "could not read runtime result {}: {source}",
                    result_path.display()
                ))
            })?;
            let raw_sha256 = if options.notefield_samples {
                let mut file = file.try_clone().map_err(|e| Error::new(e.to_string()))?;
                let mut digest = sha2::Sha256::new();
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let count = file.read(&mut buffer).map_err(|e| Error::new(e.to_string()))?;
                    if count == 0 { break; }
                    digest.update(&buffer[..count]);
                }
                // try_clone shares the file position; reopen for JSON parsing.
                Some(digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect::<String>())
            } else { None };
            let file = if options.notefield_samples {
                fs::File::open(&result_path).map_err(|e| Error::new(e.to_string()))?
            } else { file };
            let mut document: Value = serde_json::from_reader(BufReader::new(file)).map_err(|source| {
                Error::new(format!(
                    "ITGmania wrote invalid runtime JSON to {}: {source}",
                    result_path.display()
                ))
            })?;
            if let Some(message) = document.get("error").and_then(Value::as_str) {
                return Err(Error::new(format!(
                    "ITGmania probe script reported: {message}"
                )));
            }
            normalize_gameplay_clock_inputs(&mut document);
            if options.headless && !headless_graphics
                && document.pointer("/display/video_renderers").and_then(Value::as_str) != Some("null")
            {
                return Err(Error::new("ITGmania did not retain the requested null renderer"));
            }
            if headless_graphics && (document.pointer("/input/headless_graphics").and_then(Value::as_bool) != Some(true)
                || document.pointer("/input/window_visible").and_then(Value::as_bool) != Some(false))
            {
                return Err(Error::new("native graphics capture did not remain offscreen"));
            }
            if document["capabilities"]["native_fast_note_rendering"] != true
                || document["input"]["fast_note_rendering_requested"].as_bool() != options.fast_note_rendering
            {
                return Err(Error::new("native FastNoteRendering preference request was not recorded"));
            }
            if document["capabilities"]["native_song_position_clocks"] != true {
                return Err(Error::new("native raw and visible song-position clocks were not recorded"));
            }
            if document["capabilities"]["native_gameplay_clock_inputs"] != true {
                return Err(Error::new("native gameplay timer inputs were not recorded; rebuild the trace executable"));
            }
            if options.render_target_passes && document["capabilities"]["native_render_target_pass_protocol"] != 1 {
                return Err(Error::new("native render target pass observations were not recorded"));
            }
            if options.until_gameplay_end {
                validate_script_errors(&document)?;
                if document.pointer("/completion/reason").and_then(Value::as_str) != Some("gameplay-exit") {
                    return Err(Error::new("ITGmania did not reach native gameplay exit"));
                }
                if document.get("dropped_events").and_then(Value::as_u64).unwrap_or(0) != 0
                    || document.get("dropped_stage_samples").and_then(Value::as_u64).unwrap_or(0) != 0
                    || document.get("events").and_then(Value::as_array).is_some_and(|events| {
                        events.iter().any(|event| event.get("kind").and_then(Value::as_str) == Some("error"))
                    })
                {
                    return Err(Error::new("native gameplay capture contains errors or dropped observations"));
                }
            }
            if options.notefield_samples {
                if document["capabilities"]["native_option_clock_deltas"] != true {
                    return Err(Error::new("native option approach clock deltas were not recorded"));
                }
                if document["capabilities"]["native_model_render_state_protocol"] != 1
                    || document["capabilities"]["native_model_render_state_backend"] != "opengl"
                { return Err(Error::new("native Model render-state capture requires the observed OpenGL backend")); }
                validate_interactive_inputs(&document)?;
            } else {
                validate_runtime_inputs(&document)?;
                validate_native_source(&document)?;
            }
            document["source_simfile"] = Value::String(slash_path(&options.simfile));
            // The installed probe wraps Actor::addcommand unconditionally.
            // Keep that observation profile with its trace so a legacy archive
            // template cannot silently disable matching command observations.
            let capture_binary = std::env::current_exe().and_then(fs::read)
                .map_err(|source| Error::new(format!("could not hash capture executable: {source}")))?;
            document["command_observer"] = serde_json::json!({
                "dynamic_commands": true,
                "capture_binary_sha256": sha2::Sha256::digest(&capture_binary).iter()
                    .map(|byte| format!("{byte:02x}")).collect::<String>(),
            });
            if options.notefield_samples {
                // Keep this an actual native render observation. Derived actor
                // transforms cannot stand in for submitted note meshes.
                let staged_song = run_dir.join("Songs/Harness/Probe").join(simfile.file_name().unwrap());
                let provenance=serde_json::json!({
                    "raw_trace_sha256":raw_sha256,
                    "simfile_sha256":sha256_hex(fs::read(staged_song).map_err(|e|Error::new(e.to_string()))?),
                    "native_executable_sha256":sha256_hex(fs::read(&staged_binary).map_err(|e|Error::new(e.to_string()))?),
                    "observer_sha256":sha256_hex(NOTEFIELD_LUA.as_bytes()),
                    "capture_binary_sha256":document["command_observer"]["capture_binary_sha256"],
                });
                return Ok(notefield_render_document(document,provenance));
            }
            crate::song_lua_semantics::enrich(&mut document).map_err(|source| {
                Error::new(format!("could not derive song Lua semantics: {source}"))
            })?;
            return Ok(document);
        }
        let crash_report = run_dir.join("crashinfo.txt");
        if crash_report.is_file() {
            return Err(Error::new(format!(
                "ITGmania crashed before writing a semantic trace; crash report {}{}",
                crash_report.display(), log_hint(run_dir)
            )));
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|source| Error::new(format!("could not query ITGmania process: {source}")))?
        {
            return Err(Error::new(format!(
                "ITGmania exited with {status} before writing a semantic trace{}",
                log_hint(run_dir)
            )));
        }
        if started.elapsed() >= options.timeout {
            return Err(Error::new(format!(
                "ITGmania did not write a semantic trace within {:.1} seconds{}",
                options.timeout.as_secs_f64(),
                log_hint(run_dir)
            )));
        }
        thread::sleep(Duration::from_millis(25));
    })();
    // Reap the engine on every exit, including malformed output and I/O errors.
    let _ = child.kill();
    let _ = child.wait();
    result
}

fn validate_script_errors(document: &Value) -> Result<(), Error> {
    let errors = document.get("script_errors").and_then(Value::as_array)
        .ok_or_else(|| Error::new("native script-error observer is missing; rebuild the trace executable"))?;
    if document.get("dropped_script_errors").and_then(Value::as_u64) != Some(0) {
        return Err(Error::new("native script-error diagnostics are incomplete"));
    }
    if let Some(error) = errors.first() {
        return Err(Error::new(format!("native script loading or execution failed: {error}")));
    }
    Ok(())
}

fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    sha2::Sha256::digest(bytes.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn notefield_render_document(mut native: Value, provenance: Value) -> Value {
    let mut render = serde_json::Map::new();
    // Retain every submitted mesh/strip and every clock sample. Semantic method
    // tapes are a separate capture product, not a substitute for this geometry.
    for key in [
        "schema_version",
        "oracle",
        "harness_version",
        "itgmania_version",
        "source_simfile",
        "simfile",
        "title",
        "description",
        "difficulty",
        "style",
        "steps_type",
        "display",
        "theme",
        "input",
        "capture_limits",
        "completion",
        "end_position",
        "random_seed",
        "runtime_actors",
        "actor_definitions",
        "update_frames",
        "player_actor_position_samples",
        "model_geometry_meshes",
        "model_geometry_samples",
        "model_geometry_frames",
        "receptor_visits",
        "receptor_frames",
        "dropped_receptor_visits",
        "drawable_samples",
        "drawable_frames",
        "dropped_drawable_samples",
        "dropped_model_geometry_samples",
        "script_errors",
        "dropped_script_errors",
    ] {
        if let Some(value) = native.as_object_mut().unwrap().remove(key) {
            render.insert(key.into(), value);
        }
    }
    render.insert("capabilities".into(),serde_json::json!({
        "native_notefield_geometry":true,"native_drawable_vertices":true,
        "native_model_geometry_protocol":2,
        "native_model_render_state_protocol":native["capabilities"]["native_model_render_state_protocol"],
        "native_model_render_state_backend":native["capabilities"]["native_model_render_state_backend"],
        "native_receptor_geometry_protocol":1,
        "native_fast_note_rendering":native["capabilities"]["native_fast_note_rendering"],
        "native_song_position_clocks":native["capabilities"]["native_song_position_clocks"],
        "native_option_clock_deltas":native["capabilities"]["native_option_clock_deltas"],
        "native_arrow_update_clocks":native["capabilities"]["native_arrow_update_clocks"],
        "native_gameplay_clock_inputs":native["capabilities"]["native_gameplay_clock_inputs"],
        "native_player_actor_positions":native["capabilities"]["native_player_actor_positions"],
        "native_player_current_options":native["capabilities"]["native_player_current_options"],
        "native_pre_theme_preferences":native["capabilities"]["native_pre_theme_preferences"],
        "semantic_event_replay":false,"source_derived_transform_model":false,"raster_output":false,
    }));
    render.insert("provenance".into(), provenance);
    Value::Object(render)
}

fn prepare_install(
    options: &Options,
    simfile: &Path,
    binary: &Path,
    install_root: &Path,
    run_dir: &Path,
) -> Result<(), Error> {
    let data = install_root.join("Data");
    if !data.is_dir() {
        return Err(Error::new(format!(
            "{} has no Data directory; --itgmania-bin must point into a complete ITGmania install",
            install_root.display()
        )));
    }
    let fallback = install_root.join("Themes").join(&options.fallback_theme);
    if !fallback.is_dir() {
        return Err(Error::new(format!(
            "fallback theme `{}` does not exist under {}",
            options.fallback_theme,
            install_root.join("Themes").display()
        )));
    }
    if slash_path(install_root).contains(',') {
        return Err(Error::new(
            "ITGmania install paths containing commas cannot be mounted by ITGmania",
        ));
    }

    let source_program = binary
        .parent()
        .ok_or_else(|| Error::new("ITGmania executable has no parent directory"))?;
    let relative_program = source_program
        .strip_prefix(install_root)
        .map_err(|_| Error::new("ITGmania executable is not inside the inferred install root"))?;
    copy_dir(
        source_program,
        &run_dir.join(relative_program),
        cfg!(windows),
        None,
    )?;
    copy_dir(&data, &run_dir.join("Data"), true, None)?;
    copy_dir(
        simfile
            .parent()
            .ok_or_else(|| Error::new("simfile has no song directory"))?,
        &run_dir.join("Songs/Harness/Probe"),
        true,
        Some(simfile),
    )?;
    copy_dir(
        &fallback.join("Scripts"),
        &run_dir.join("Themes/itgmania-harness/Scripts"),
        true,
        None,
    )?;
    // SL-Helpers scans GetCurrentThemeDirectory(), rather than searching the
    // fallback chain. Its combo/judgment catalogs need the same asset namespace
    // as the scripts staged above; otherwise valid fonts disappear in probes.
    for directory in ["Fonts", "Graphics", "Other", "Sounds"] {
        let source = fallback.join(directory);
        if source.is_dir() {
            copy_dir(&source, &run_dir.join("Themes/itgmania-harness").join(directory), true, None)?;
        }
    }

    for directory in [
        "Cache",
        "Logs",
        "Save/itgmania-harness",
        "Screenshots",
        "Themes/itgmania-harness/BGAnimations/ScreenSongLuaHarness overlay",
        "Themes/itgmania-harness/BGAnimations",
        "Themes/itgmania-harness/Scripts",
    ] {
        create_dir(&run_dir.join(directory))?;
    }
    let portable = run_dir.join("Portable.ini");
    if portable.is_file() {
        fs::remove_file(&portable)
            .map_err(|error| io_error("replace staged Portable.ini", &portable, error))?;
    }
    write(&portable, b"[Options]\n")?;
    let mut native_preferences = preferences(install_root, options.headless, options.drawable_samples)?;
    if let Some(fps) = options.fixed_update_fps {
        // Stock GameLoop preference: keep observer overhead from amplifying
        // self-queued callbacks into an unbounded catch-up update.
        native_preferences.push_str(&format!("ConstantUpdateDeltaSeconds={:.17}\n", 1.0 / f64::from(fps)));
    }
    if let Some(enabled) = options.fast_note_rendering {
        native_preferences.push_str(&format!("FastNoteRendering={}\n", u8::from(enabled)));
    }
    write(&run_dir.join("Save/Preferences.ini"), native_preferences.as_bytes())?;
    write(
        &run_dir.join("Themes/itgmania-harness/ThemeInfo.ini"),
        b"[ThemeInfo]\nDisplayName=ITGmania Harness\nAuthor=deadsync\n",
    )?;
    write(
        &run_dir.join("Themes/itgmania-harness/metrics.ini"),
        metrics(&options.fallback_theme).as_bytes(),
    )?;
    write(
        &run_dir.join("Themes/itgmania-harness/Scripts/00 HarnessConfig.lua"),
        config_lua(options).as_bytes(),
    )?;
    let trace_lua = if options.notefield_samples {
        NOTEFIELD_LUA.to_owned()
    } else {
        format!("{TRACE_JSON_LUA}\n{TRACE_LUA}")
    };
    write(
        &run_dir.join("Themes/itgmania-harness/Scripts/03 SongLuaTrace.lua"),
        trace_lua.as_bytes(),
    )?;
    write(
        &run_dir
            .join("Themes/itgmania-harness/BGAnimations/ScreenSongLuaHarness overlay/default.lua"),
        INIT_LUA.as_bytes(),
    )?;
    write(
        &run_dir.join("Themes/itgmania-harness/BGAnimations/ScreenGameplay overlay.lua"),
        GAMEPLAY_LUA.as_bytes(),
    )?;
    write(
        &run_dir.join("Themes/itgmania-harness/BGAnimations/ScreenSystemLayer overlay.lua"),
        SYSTEM_LUA.as_bytes(),
    )?;
    Ok(())
}

fn install_root(binary: &Path) -> Result<PathBuf, Error> {
    let parent = binary
        .parent()
        .ok_or_else(|| Error::new("ITGmania executable has no parent directory"))?;
    if cfg!(windows)
        && parent
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("Program"))
    {
        return parent
            .parent()
            .map(Path::to_owned)
            .ok_or_else(|| Error::new("ITGmania Program directory has no install root"));
    }
    Ok(parent.to_owned())
}

fn require_headless_graphics_adapter(binary: &[u8]) -> Result<(), Error> {
    let marker = b"ITGMANIA_HARNESS_HEADLESS_GRAPHICS_AVAILABLE";
    if binary.windows(marker.len()).any(|bytes| bytes == marker) {
        Ok(())
    } else {
        Err(Error::new("headless graphics requires the offscreen trace adapter; refusing to open a visible window"))
    }
}

fn preferences(install_root: &Path, headless: bool, drawable_samples: bool) -> Result<String, Error> {
    // CheckVideoDefaultSettings replaces VideoRenderers when this differs.
    let renderer = if headless {
        let renderer = if drawable_samples { "opengl" } else { "null" };
        format!("VideoRenderers={renderer}\nLastSeenVideoDriver={}\nShowLoadingWindow=0\n", video_driver()?)
    } else {
        String::new()
    };
    Ok(format!(
        "[Options]\n\
         AdditionalFoldersReadOnly={}\n\
         AllowMultipleInstances=1\n\
         FastLoad=0\n\
         LogToDisk=1\n\
         ShowLogOutput=0\n\
         SoundDrivers=Null\n\
         {}\
         TestInitialScreen=ScreenSongLuaHarness\n\
         Theme={THEME_NAME}\n\
         Windowed=1\n\
         DisplayWidth=854\n\
         DisplayHeight=480\n",
        slash_path(install_root),
        renderer
    ))
}

#[cfg(not(windows))]
fn video_driver() -> Result<String, Error> {
    // StepMania.cpp::GetVideoDriverName uses this literal outside Windows.
    Ok("OpenGL".into())
}

#[cfg(windows)]
fn video_driver() -> Result<String, Error> {
    // Match VideoDriverInfo.cpp::GetPrimaryVideoName, including the ANSI API.
    #[repr(C)]
    struct DisplayDevice {
        size: u32,
        name: [u8; 32],
        description: [u8; 128],
        flags: u32,
        id: [u8; 128],
        key: [u8; 128],
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumDisplayDevicesA(device: *const u8, index: u32, out: *mut DisplayDevice, flags: u32) -> i32;
    }
    for index in 0.. {
        let mut device = DisplayDevice {
            size: std::mem::size_of::<DisplayDevice>() as u32,
            name: [0; 32], description: [0; 128], flags: 0, id: [0; 128], key: [0; 128],
        };
        // SAFETY: the struct has the DISPLAY_DEVICEA layout, its cb is set,
        // and the API writes only to this live buffer; a null device enumerates adapters.
        if unsafe { EnumDisplayDevicesA(std::ptr::null(), index, &mut device, 0) } == 0 {
            return Err(Error::new("could not identify the primary video driver for headless capture"));
        }
        if device.flags & 4 != 0 {
            let end = device.description.iter().position(|byte| *byte == 0).unwrap_or(device.description.len());
            let name = std::str::from_utf8(&device.description[..end])
                .map_err(|_| Error::new("primary video driver name is not UTF-8"))?;
            return Ok(name.trim_end().to_owned());
        }
    }
    unreachable!("unbounded adapter enumeration returns at its end")
}

fn metrics(fallback_theme: &str) -> String {
    format!(
        "[Global]\n\
         FallbackTheme={}\n\n\
         [Common]\n\
         InitialScreen=\"ScreenSongLuaHarness\"\n\n\
         [ScreenSongLuaHarness]\n\
         Class=\"ScreenWithMenuElements\"\n\
         Fallback=\"ScreenWithMenuElements\"\n\
         NextScreen=\"ScreenGameplay\"\n\
         PrevScreen=\"ScreenSongLuaHarness\"\n\
         PlayMusic=false\n\
         ShowCreditDisplay=false\n\
         TimerSeconds=-1\n",
        fallback_theme
    )
}

pub(crate) fn validate_runtime_inputs(document: &Value) -> Result<(), Error> {
    if document.get("script_errors").is_some() || document.get("dropped_script_errors").is_some() {
        validate_script_errors(document)?;
    }
    validate_interactive_inputs(document)?;
    if document.pointer("/capabilities/native_source_provenance") == Some(&Value::Bool(true))
        || document.pointer("/input/native_source").is_some()
    {
        validate_native_source(document)?;
    }
    if document.pointer("/capabilities/native_random_seed").and_then(Value::as_bool) == Some(true)
        && (!document["random_seed"].as_u64().is_some_and(|seed| seed > 0 && seed <= i32::MAX as u64)
            || !document["random_reseeds"].is_array()
            || document["dropped_random_reseeds"] != 0)
    {
        return Err(Error::new("native Lua random inputs are missing or incomplete"));
    }
    validate_actor_identities(document)?;
    validate_actor_creations(document)?;
    validate_declaration_forest(document)?;
    validate_actor_queries(document)?;
    validate_gameplay_preferences(document)?;
    validate_theme_inputs(document)?;
    let preferences = document.pointer("/input/preferences").ok_or_else(|| {
        Error::new("native gameplay capture has no machine preferences")
    })?;
    if document.pointer("/capabilities/native_machine_preferences").and_then(Value::as_bool) != Some(true)
        || !["global_offset_seconds", "visual_delay_seconds"].into_iter().all(|key| {
            preferences.get(key).and_then(Value::as_f64).is_some_and(f64::is_finite)
        })
        || !matches!(preferences.get("default_sync_offset").and_then(Value::as_str),
            Some("SyncOffset_NULL" | "SyncOffset_ITG"))
    {
        return Err(Error::new("native gameplay machine preferences are incomplete"));
    }
    if document.pointer("/capabilities/native_display_preference").and_then(Value::as_bool) == Some(true)
        && !preferences.get("display_aspect_ratio").and_then(Value::as_f64)
            .is_some_and(|value| value.is_finite() && value >= 0.0)
    {
        return Err(Error::new("native configured display aspect ratio is missing or invalid"));
    }
    let frames = document.get("update_frames").and_then(Value::as_array)
        .filter(|frames| !frames.is_empty())
        .ok_or_else(|| Error::new("native gameplay capture has no update-frame clocks"))?;
    if document.pointer("/capabilities/native_update_frames").and_then(Value::as_bool) != Some(true)
        || document.get("dropped_update_frames").and_then(Value::as_u64) != Some(0)
        || document.get("update_frame_phase").and_then(Value::as_str) != Some("gameplay-overlay-probe")
    {
        return Err(Error::new("native gameplay update-frame capture is incomplete"));
    }
    let mut previous_scene = 0;
    let mut previous_wall = 0.0;
    for (index, frame) in frames.iter().enumerate() {
        let scene = frame.get("scene").and_then(Value::as_u64).unwrap_or(0);
        let finite = |key| frame.get(key).and_then(Value::as_f64).filter(|value| value.is_finite());
        let valid = frame.get("sequence").and_then(Value::as_u64) == Some(index as u64 + 1)
            && scene > 0 && scene >= previous_scene
            && finite("delta").is_some_and(|delta| delta >= 0.0)
            && finite("seconds").is_some() && finite("beat").is_some()
            && finite("wall_seconds").is_some_and(|wall| wall >= previous_wall);
        if !valid { return Err(Error::new(format!("invalid native gameplay update-frame clock at {}", index + 1))); }
        previous_scene = scene;
        // The validation above proves this finite clock exists.
        previous_wall = finite("wall_seconds").expect("validated native wall clock");
    }
    if document.pointer("/capabilities/native_event_scenes").and_then(Value::as_bool) == Some(true) {
        let events = document.get("events").and_then(Value::as_array)
            .ok_or_else(|| Error::new("native event scene capture has no events array"))?;
        let mut prior_scene = 0;
        for (index, event) in events.iter().enumerate() {
            let scene = event.get("scene").and_then(Value::as_u64)
                .filter(|scene| *scene >= prior_scene && *scene <= previous_scene);
            if event.get("seq").and_then(Value::as_u64) != Some(index as u64 + 1) || scene.is_none() {
                return Err(Error::new(format!("invalid native event scene at {}", index + 1)));
            }
            prior_scene = scene.expect("validated native event scene");
        }
    }
    if document.pointer("/capabilities/native_call_sources").and_then(Value::as_bool) == Some(true) {
        let events = document.get("events").and_then(Value::as_array)
            .ok_or_else(|| Error::new("native call source capture has no events array"))?;
        for event in events {
            if !matches!(event.get("kind").and_then(Value::as_str), Some("call" | "query" | "message" | "modifier")) {
                continue;
            }
            let source = event.pointer("/caller/source").and_then(Value::as_str);
            let line = event.pointer("/caller/line").and_then(Value::as_i64);
            // NoteSkinManager::GetMetricA compiles ActorUtil::ParseActorCommands
            // with its default empty chunk name. Lua's caller source is then
            // the empty string, which is distinct from a missing observation.
            if source.is_none() || line.is_none_or(|line| line < -1) {
                return Err(Error::new("native method event has no valid immediate caller"));
            }
        }
    }
    if document.pointer("/capabilities/native_constructor_queries").and_then(Value::as_bool) == Some(true) {
        if document.pointer("/capabilities/native_call_sources").and_then(Value::as_bool) != Some(true) {
            return Err(Error::new("native construction capture requires immediate callers"));
        }
        for event in document.get("events").and_then(Value::as_array).into_iter().flatten() {
            if !matches!(event.get("kind").and_then(Value::as_str), Some("call" | "query" | "message" | "modifier")) {
                continue;
            }
            if !matches!(event.get("phase").and_then(Value::as_str), Some("construction" | "callback")) {
                return Err(Error::new("native method capture has no construction/callback phase"));
            }
            if (event.get("kind").and_then(Value::as_str) == Some("query")
                || document.pointer("/capabilities/native_modifier_results").and_then(Value::as_bool) == Some(true)
                    && event.get("kind").and_then(Value::as_str) == Some("modifier"))
                && event.pointer("/detail/results").and_then(Value::as_array).is_none()
            {
                return Err(Error::new("native query or modifier capture has no result tuple"));
            }
        }
    }
    if let Some(classes) = document.pointer("/capabilities/native_query_classes") {
        let classes = classes.as_array().filter(|classes| !classes.is_empty())
            .ok_or_else(|| Error::new("native query classes must be a nonempty array"))?;
        for (index, class) in classes.iter().enumerate() {
            if class.as_str().is_none_or(str::is_empty) || classes[..index].contains(class) {
                return Err(Error::new("native query classes contain an invalid or duplicate class"));
            }
        }
    }
    Ok(())
}

fn validate_native_source(document: &Value) -> Result<(), Error> {
    let source = &document["input"]["native_source"];
    for (key, length) in [("revision", 40), ("note_display_sha256", 64), ("cubic_spline_sha256", 64)] {
        if source[key].as_str().is_none_or(|value| {
            value.len() != length || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(Error::new("native source provenance is missing or invalid; rebuild the trace executable"));
        }
    }
    if source["modified_src"].as_bool().is_none() {
        return Err(Error::new("native source modification state is missing"));
    }
    Ok(())
}

fn validate_notefield_geometry(document: &Value) -> Result<(), Error> {
    validate_script_errors(document)?;
    validate_native_source(document)?;
    if document["capabilities"]["native_notefield_geometry"] != true
        || document["capabilities"]["native_model_geometry_protocol"] != 2
        || document["dropped_model_geometry_samples"] != 0
    {
        return Err(Error::new(
            "native notefield geometry capture is incomplete",
        ));
    }
    let samples = document["model_geometry_samples"]
        .as_array()
        .ok_or_else(|| Error::new("native model geometry samples missing"))?;
    let meshes = document["model_geometry_meshes"]
        .as_array()
        .ok_or_else(|| Error::new("native model mesh catalog missing"))?;
    let mut ids = std::collections::BTreeSet::new();
    for mesh in meshes {
        let rows = mesh["vertices"]
            .as_array()
            .ok_or_else(|| Error::new("native model mesh vertices missing"))?;
        let id = mesh["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| Error::new("native model mesh ID missing"))?;
        if !ids.insert(id)
            || rows.is_empty()
            || rows.len() % 3 != 0
            || !rows.iter().all(|row| {
                row.as_array().is_some_and(|row| {
                    row.len() == 3
                        && row
                            .iter()
                            .all(|value| value.as_f64().is_some_and(f64::is_finite))
                })
            })
        {
            return Err(Error::new("native model mesh is incomplete"));
        }
    }
    let frames = document["model_geometry_frames"]
        .as_array()
        .filter(|frames| !frames.is_empty())
        .ok_or_else(|| Error::new("native model geometry frame boundaries missing"))?;
    let updates = document["update_frames"]
        .as_array()
        .ok_or_else(|| Error::new("native model update anchors missing"))?;
    let mut consumed = 0;
    let mut previous = 0;
    for frame in frames {
        let at = frame["update_sequence"].as_u64().unwrap_or(0);
        let count = frame["samples"].as_u64().unwrap_or(u64::MAX);
        if at == 0
            || at < previous
            || at > updates.len() as u64
            || count < consumed
            || count > samples.len() as u64
        {
            return Err(Error::new("native model frame boundary is invalid"));
        }
        for sample in &samples[consumed as usize..count as usize] {
            if sample["update_sequence"] != at
                || sample["scene"] != updates[at as usize - 1]["scene"]
                || sample["coordinate_space"] != "native-projection-before-display-centering"
                || sample["primitive"] != "ModelTriangles"
                || sample["actor"].as_str().is_none_or(str::is_empty)
                || !matches!(
                    sample["draw_phase"].as_str(),
                    Some("during_update" | "screen_draw")
                )
                || sample["state"]["render_target"].as_u64().is_none()
                || sample["state"]["anchors_clip"]
                    .as_array()
                    .is_none_or(|anchors| {
                        anchors.len() != 4
                            || !anchors.iter().all(|anchor| {
                                anchor.as_array().is_some_and(|row| {
                                    row.len() == 4
                                        && row
                                            .iter()
                                            .all(|value| value.as_f64().is_some_and(f64::is_finite))
                                })
                            })
                    })
                || sample["geometry"]
                    .as_u64()
                    .is_none_or(|id| !ids.contains(&id))
            {
                return Err(Error::new(
                    "native model geometry or frame anchor is invalid",
                ));
            }
        }
        consumed = count;
        previous = at;
    }
    if consumed != samples.len() as u64 {
        return Err(Error::new("native model geometry tail was not drawn"));
    }

    validate_model_render_state(document)?;
    validate_receptor_geometry(document)?;
    Ok(())
}

fn validate_model_render_state(document: &Value) -> Result<(), Error> {
    let capability = &document["capabilities"]["native_model_render_state_protocol"];
    // Retained geometry-only documents do not acquire rendering evidence.
    // Fresh captures require the protocol separately before this validator.
    if capability.is_null() { return Ok(()); }
    let invalid = || Error::new("native Model render state is missing or invalid");
    if capability != 1 || document["capabilities"]["native_model_render_state_backend"] != "opengl" {
        return Err(invalid());
    }
    let finite = |value: &Value| value.as_f64().is_some_and(f64::is_finite);
    let vector = |value: &Value, length| value.as_array().is_some_and(|values|
        values.len() == length && values.iter().all(&finite));
    for sample in document["model_geometry_samples"].as_array().ok_or_else(invalid)? {
        let state = &sample["state"]["render_state"];
        if state["backend"] != "opengl" || state["lighting"].as_bool().is_none()
            || !vector(&state["current_color"], 4)
            || ["emissive", "ambient", "diffuse", "specular"].iter()
                .any(|name| !vector(&state["material"][name], 4))
            || !finite(&state["material"]["shininess"])
            || state["blend"]["enabled"].as_bool().is_none()
            || ["src_rgb", "dst_rgb", "src_alpha", "dst_alpha", "equation_rgb", "equation_alpha"].iter()
                .any(|name| state["blend"][name].as_u64().is_none())
            || state["depth"]["test_enabled"].as_bool().is_none()
            || state["depth"]["write"].as_bool().is_none()
            || state["depth"]["function"].as_u64().is_none()
            || !vector(&state["depth"]["range"], 2)
            || state["cull"]["enabled"].as_bool().is_none()
            || state["cull"]["face"].as_u64().is_none()
            || state["alpha_test"]["enabled"].as_bool().is_none()
            || state["alpha_test"]["function"].as_u64().is_none()
            || !finite(&state["alpha_test"]["reference"])
        { return Err(invalid()); }
        let textures = state["textures"].as_array().filter(|textures| (1..=4).contains(&textures.len()))
            .ok_or_else(invalid)?;
        for (unit, texture) in textures.iter().enumerate() {
            let enabled = texture["enabled"].as_bool().ok_or_else(invalid)?;
            if texture["unit"] != unit || texture["handle"].as_u64().is_none()
                || texture["environment_mode"].as_u64().is_none()
                || texture["mode"].as_i64().is_none()
                || enabled && (texture["handle"] == 0
                    || texture["mode"].as_i64().is_none_or(|mode| !(0..=2).contains(&mode))
                    || texture["texture"].as_str().is_none_or(str::is_empty))
            { return Err(invalid()); }
        }
    }
    Ok(())
}

fn validate_receptor_geometry(document: &Value) -> Result<(), Error> {
    let invalid = || Error::new("native receptor visits or draw associations are incomplete");
    if document["capabilities"]["native_receptor_geometry_protocol"] != 1
        || document["dropped_receptor_visits"] != 0
        || !document["input"]["native_source"]["receptor_arrow_row_sha256"]
            .as_str().is_some_and(|value| value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit()))
    { return Err(invalid()); }
    let visits = document["receptor_visits"].as_array().ok_or_else(invalid)?;
    let frames = document["receptor_frames"].as_array().ok_or_else(invalid)?;
    let draw_frames = document["model_geometry_frames"].as_array().ok_or_else(invalid)?;
    let updates = document["update_frames"].as_array().ok_or_else(invalid)?;
    if frames.len() != draw_frames.len() || frames.is_empty() { return Err(invalid()); }
    let mut by_sequence = std::collections::BTreeMap::new();
    let mut consumed = 0;
    let mut previous_sequence = 0;
    for (frame, draw_frame) in frames.iter().zip(draw_frames) {
        let at = frame["update_sequence"].as_u64().unwrap_or(0);
        let count = frame["samples"].as_u64().unwrap_or(u64::MAX);
        if at == 0 || at > updates.len() as u64 || frame["update_sequence"] != draw_frame["update_sequence"]
            || count < consumed || count > visits.len() as u64
        { return Err(invalid()); }
        for visit in &visits[consumed as usize..count as usize] {
            let state = &visit["state"];
            let context = &state["note_context"];
            let sequence = context["receptor_visit_sequence"].as_u64().unwrap_or(0);
            if visit["update_sequence"] != at || visit["scene"] != updates[at as usize - 1]["scene"]
                || visit["actor"].as_str().is_none_or(str::is_empty)
                || sequence <= previous_sequence || context["kind"] != "receptor"
                || context["player"].as_u64().is_none_or(|player| player > 1)
                || context["column"].as_u64().is_none()
                || context["draw_beat"].as_f64().is_none_or(|beat| !beat.is_finite())
                || state["base_alpha"].as_f64().is_none_or(|alpha| !(0.0..=1.0).contains(&alpha))
                || state["visible"].as_bool().is_none() || state["render_target"].as_u64().is_none()
                || !matches!(state["draw_phase"].as_str(), Some("during_update" | "screen_draw"))
            { return Err(invalid()); }
            previous_sequence = sequence;
            by_sequence.insert(sequence, visit);
        }
        consumed = count;
    }
    if consumed != visits.len() as u64 { return Err(invalid()); }
    for key in ["drawable_samples", "model_geometry_samples"] {
        for sample in document[key].as_array().ok_or_else(invalid)? {
            let state = &sample["state"];
            let context = &state["note_context"];
            if context["kind"] != "receptor" { continue; }
            let sequence = context["receptor_visit_sequence"].as_u64().ok_or_else(invalid)?;
            let visit = by_sequence.get(&sequence).ok_or_else(invalid)?;
            if sample["scene"] != visit["scene"] || sample["update_sequence"] != visit["update_sequence"]
                || state["render_target"] != visit["state"]["render_target"]
                || context != &visit["state"]["note_context"]
            { return Err(invalid()); }
        }
    }
    if document["input"]["scene_draws"] == true {
        let mut seen = std::collections::BTreeSet::new();
        for key in ["drawable_samples", "model_geometry_samples"] {
            let mut previous = 0;
            for sample in document[key].as_array().ok_or_else(invalid)? {
                let sequence = sample["state"]["native_draw_sequence"].as_u64().unwrap_or(0);
                if sequence <= previous || !seen.insert(sequence) { return Err(invalid()); }
                previous = sequence;
            }
        }
    }
    Ok(())
}

fn validate_render_random_inputs(document: &Value) -> Result<(), Error> {
    let advertised = document["capabilities"]["native_render_random_inputs"] == true;
    let invalid = || Error::new("native render random inputs or scope associations are incomplete");
    if !advertised {
        if !document["input"]["render_random"].is_null()
            || document["render_random_inputs"].as_array().is_some_and(|inputs| !inputs.is_empty())
            || document["drawable_samples"].as_array().is_some_and(|samples| samples.iter()
                .any(|sample| sample.get("render_random_sequences").is_some()))
        { return Err(invalid()); }
        return Ok(());
    }
    let provenance = &document["input"]["render_random"];
    if document["capabilities"]["native_drawable_vertices"] != true
        || document["dropped_render_random_inputs"] != 0
        || provenance["protocol"] != 1
        || provenance["observation"] != "native-Actor-PreDraw-vibrate-randomf"
        || provenance["generator"] != "std::mt19937"
        || provenance["distribution"] != "uniform_real_distribution<double>(-1,1)-to-f32"
        || provenance["seed_source"] != "native-default-time"
        || provenance["association"] != "active-Actor-Draw-PreDraw-scopes"
    { return Err(invalid()); }
    for key in ["actor_sha256", "random_numbers_sha256", "random_numbers_header_sha256"] {
        if !document["input"]["native_source"][key].as_str().is_some_and(|hash|
            hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
        { return Err(Error::new("native render random source provenance is missing")); }
    }
    let inputs = document["render_random_inputs"].as_array().ok_or_else(invalid)?;
    let frames = document["update_frames"].as_array().ok_or_else(invalid)?;
    let samples = document["drawable_samples"].as_array().ok_or_else(invalid)?;
    let actors = document["runtime_actors"].as_array().ok_or_else(invalid)?;
    let mut actor_by_id = std::collections::HashMap::new();
    for actor in actors {
        let id = actor["id"].as_str().filter(|id| !id.is_empty()).ok_or_else(invalid)?;
        if actor_by_id.insert(id, actor).is_some() { return Err(invalid()); }
    }
    let identity_matches = |input: &Value, id_key: &str, definition_key: &str, path_key: &str| {
        input[id_key].as_str().and_then(|id| actor_by_id.get(id)).is_some_and(|actor| {
            input[definition_key] == actor["definition_id"]
                && input[path_key] == actor["path"]
                && input[path_key].as_str().is_some_and(|path| !path.is_empty())
        })
    };
    let mut native_scopes = std::collections::HashSet::new();
    let mut predraw_occurrences = std::collections::HashSet::new();
    let mut draw_roles = std::collections::HashSet::new();
    let mut draw_owners = std::collections::HashMap::new();
    let mut draw_occurrences = std::collections::HashMap::new();
    let mut previous_update = 0;
    for (index, input) in inputs.iter().enumerate() {
        let at = input["update_sequence"].as_u64().unwrap_or(0);
        let native_draw = input["native_draw_sequence"].as_u64().unwrap_or(0);
        let native_update = input["native_update_sequence"].as_u64().unwrap_or(0);
        let draw_ordinal = input["draw_ordinal"].as_u64().unwrap_or(0);
        let predraw_ordinal = input["predraw_ordinal"].as_u64().unwrap_or(0);
        let wrapper = input["wrapper_index"].as_i64().unwrap_or(i64::MIN);
        let owner = input["actor"].as_str().unwrap_or("");
        let scope = input["native_scope_sequence"].as_u64().unwrap_or(0);
        if input["sequence"].as_u64() != Some(index as u64 + 1)
            || at == 0 || at < previous_update || at > frames.len() as u64
            || input["scene"] != frames[at.saturating_sub(1) as usize]["scene"]
            || native_draw == 0 || native_update == 0 || draw_ordinal == 0 || predraw_ordinal == 0
            || scope == 0 || !native_scopes.insert(scope) || wrapper < -1
            || !predraw_occurrences.insert((native_draw, predraw_ordinal))
            || !draw_roles.insert((native_draw, wrapper))
            || !identity_matches(input, "actor", "definition_id", "path")
            || !input["values"].as_array().is_some_and(|values| values.len() == 3
                && values.iter().all(|value| value.as_f64().is_some_and(|value|
                    value.is_finite() && (-1.0..=1.0).contains(&value)
                        && f64::from(value as f32) == value)))
        { return Err(invalid()); }
        let draw_owner = (owner, native_update, draw_ordinal);
        if draw_owners.insert(native_draw, draw_owner).is_some_and(|previous| previous != draw_owner) {
            return Err(invalid());
        }
        if draw_occurrences.insert(draw_owner, native_draw).is_some_and(|previous| previous != native_draw) {
            return Err(invalid());
        }
        if input["source_actor"].is_null() {
            if wrapper <= 0 || !input["source_definition_id"].is_null() || !input["source_path"].is_null() {
                return Err(invalid());
            }
        } else if !identity_matches(input, "source_actor", "source_definition_id", "source_path")
            || wrapper == 0 && input["source_actor"] != input["actor"]
        { return Err(invalid()); }
        previous_update = at;
    }
    let mut used = vec![false; inputs.len()];
    for sample in samples {
        let sequences = sample["render_random_sequences"].as_array().ok_or_else(invalid)?;
        let mut previous = 0;
        for sequence in sequences {
            let sequence = sequence.as_u64().unwrap_or(0);
            if sequence <= previous || sequence > inputs.len() as u64 { return Err(invalid()); }
            let input = &inputs[sequence as usize - 1];
            if input["scene"] != sample["scene"] || input["update_sequence"] != sample["update_sequence"] {
                return Err(invalid());
            }
            used[sequence as usize - 1] = true;
            previous = sequence;
        }
    }
    if used.iter().any(|used| !used) { return Err(invalid()); }
    let mut consumed = 0;
    for frame in document["drawable_frames"].as_array().ok_or_else(invalid)? {
        let count = frame["render_random_inputs"].as_u64().ok_or_else(invalid)?;
        if count < consumed || count > inputs.len() as u64
            || inputs[consumed as usize..count as usize].iter().any(|input|
                input["update_sequence"] != frame["update_sequence"] || input["scene"] != frame["scene"])
        { return Err(invalid()); }
        consumed = count;
    }
    if consumed != inputs.len() as u64 { return Err(invalid()); }
    Ok(())
}

fn validate_fast_note_rendering(document: &Value) -> Result<(), Error> {
    if document["capabilities"]["native_fast_note_rendering"] != true {
        return Ok(());
    }
    let input = &document["input"];
    let requested = &input["fast_note_rendering_requested"];
    let initial = input["fast_note_rendering"].as_bool();
    if initial.is_none() || !input["fast_note_rendering_at_end"].is_boolean()
        || !requested.is_null() && !requested.is_boolean()
        || requested.as_bool().is_some_and(|value| Some(value) != initial)
    {
        return Err(Error::new("native FastNoteRendering preference inputs are missing or do not match the request"));
    }
    Ok(())
}

fn validate_song_position_clocks(document: &Value) -> Result<(), Error> {
    if document["capabilities"]["native_song_position_clocks"] != true {
        return Ok(());
    }
    let geometry = document["input"]["notefield_samples"] == true;
    let alias = if geometry { "visible_beat" } else { "song_beat" };
    let valid_clock = |clock: &Value| {
        ["seconds", "beat", "song_beat", "visible_seconds", "visible_beat"].into_iter()
            .all(|key| clock[key].as_f64().is_some_and(f64::is_finite))
            && clock["beat"].as_f64() == clock[alias].as_f64()
    };
    let frames = document["update_frames"].as_array();
    if !frames.is_some_and(|frames| !frames.is_empty() && frames.iter().all(valid_clock))
        || !valid_clock(&document["end_position"])
    {
        return Err(Error::new("native song-position clocks are missing, non-finite, or use the wrong legacy beat alias"));
    }
    let preferences = &document["input"]["preferences"];
    if !["global_offset_seconds", "visual_delay_seconds", "music_rate"].into_iter()
        .all(|key| preferences[key].as_f64().is_some_and(f64::is_finite))
        || !matches!(preferences["default_sync_offset"].as_str(), Some("SyncOffset_NULL" | "SyncOffset_ITG"))
    {
        return Err(Error::new("native timing preference inputs are missing or invalid"));
    }
    let players = if geometry { &document["input"]["players"] }
        else { &document["input"]["gameplay_preferences"]["players"] };
    if !players.as_array().is_some_and(|players| !players.is_empty() && players.iter().all(|player|
        player["preferred_visual_delay_seconds"].as_f64().is_some_and(f64::is_finite)))
    {
        return Err(Error::new("native preferred player visual delays are missing or invalid"));
    }
    if document["capabilities"]["native_pre_theme_preferences"] == true {
        let catalog = &document["input"]["catalog_preferences"];
        if catalog["initialization"] != "native-catalog-note-time-calculation"
            || !["global_offset_seconds", "music_rate"].into_iter()
                .all(|key| catalog[key].as_f64().is_some_and(f64::is_finite))
        {
            return Err(Error::new("native catalog timing preference inputs are missing or invalid"));
        }
    }
    Ok(())
}

fn validate_option_clock_deltas(document: &Value) -> Result<(), Error> {
    let capability = &document["capabilities"]["native_option_clock_deltas"];
    // Retained traces without this capability cannot acquire timer evidence.
    if capability.is_null() {
        return Ok(());
    }
    let valid_delta = |value: &Value| value.as_f64().is_some_and(|delta|
        delta.is_finite() && delta >= 0.0 && f64::from(delta as f32) == delta);
    let frames = document["update_frames"].as_array();
    if capability != true || !frames.is_some_and(|frames| !frames.is_empty()
        && frames.iter().all(|frame| {
            let deltas = &frame["option_deltas"];
            valid_delta(&deltas["song"])
                && deltas["players"].as_array().is_some_and(|players|
                    players.len() == 2 && players.iter().all(valid_delta))
        }))
    {
        return Err(Error::new("native option approach clocks require a finite nonnegative f32 delta for song and both players on every frame"));
    }
    Ok(())
}

fn normalize_gameplay_clock_inputs(document: &mut Value) {
    if document["capabilities"]["native_gameplay_clock_inputs"] != true { return; }
    // JsonEncode cannot retain a Lua object key whose value is nil. Convert
    // only the recorder's explicit no-call sentinel; never fill an absent key.
    if let Some(frames) = document["update_frames"].as_array_mut() {
        for frame in frames {
            if frame.get("gameplay_clock_inputs") == Some(&Value::Bool(false)) {
                frame["gameplay_clock_inputs"] = Value::Null;
            }
        }
    }
}

fn validate_gameplay_clock_inputs(document: &Value) -> Result<(), Error> {
    let capability = &document["capabilities"]["native_gameplay_clock_inputs"];
    if capability.is_null() { return Ok(()); }
    let invalid = || Error::new("native gameplay timer inputs or their frame associations are incomplete");
    let frames = document["update_frames"].as_array().ok_or_else(invalid)?;
    if capability != true || frames.is_empty() { return Err(invalid()); }
    if !document["input"]["native_source"]["screen_gameplay_sha256"].as_str()
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
    { return Err(Error::new("native gameplay timer source provenance is missing")); }
    let exact_f32 = |value: &Value| value.as_f64()
        .is_some_and(|number| number.is_finite() && f64::from(number as f32) == number);
    let mut recorded_updates = std::collections::BTreeSet::new();
    for (index, frame) in frames.iter().enumerate() {
        let scene = frame["scene"].as_u64().filter(|scene| *scene > 0).ok_or_else(invalid)?;
        let native_frame = frame["native_update_frame"].as_u64().ok_or_else(invalid)?;
        let native_sequence = frame["native_update_sequence"].as_u64().ok_or_else(invalid)?;
        if frame["sequence"].as_u64() != Some(index as u64 + 1) { return Err(invalid()); }
        let clock = frame.get("gameplay_clock_inputs").ok_or_else(invalid)?;
        // No getter call is a real input state: intro, pause and outro skip it.
        if clock.is_null() { continue; }
        if !clock.as_object().is_some_and(|fields| fields.len() == 7)
            || clock["scene"].as_u64() != Some(scene)
            || clock["native_update_frame"].as_u64() != Some(native_frame)
            || clock["native_update_sequence"].as_u64() != Some(native_sequence)
            || native_sequence == 0 || !recorded_updates.insert(native_sequence)
            || clock["phase"] != "after-gameplay-actors"
            || !exact_f32(&clock["delta_seconds"])
            || clock["delta_seconds"].as_f64().is_none_or(|delta| delta < 0.0)
            || !exact_f32(&clock["music_rate"])
            || !clock["steps_time_active"].is_boolean()
        { return Err(invalid()); }
    }
    Ok(())
}

fn validate_arrow_update_clocks(document: &Value) -> Result<(), Error> {
    let capability = &document["capabilities"]["native_arrow_update_clocks"];
    if capability.is_null() { return Ok(()); }
    let invalid = || Error::new("native ArrowEffects cache clocks are missing or malformed");
    let frames = document["update_frames"].as_array().ok_or_else(invalid)?;
    if capability != true || frames.is_empty() { return Err(invalid()); }
    let valid_time = |value: &Value| value.as_f64()
        .is_some_and(|time| time.is_finite() && time >= 0.0);
    for frame in frames {
        let clocks = &frame["arrow_clock_inputs"];
        // Screen construction may produce a preroll sample before its first
        // ArrowEffects::Update. Active frames must have their own cache input.
        if clocks.is_null() && frame["song_beat"].as_f64().is_some_and(|beat| beat < 0.0) {
            continue;
        }
        if !valid_time(&clocks["update_seconds"])
            || !clocks["players"].as_array().is_some_and(|players| players.len() == 2
                && players.iter().all(|player| {
                    // JsonEncode may encode a native empty Lua table as [].
                    (player.is_object() || player.as_array().is_some_and(Vec::is_empty))
                        && ["tipsy_seconds", "tan_tipsy_seconds"].into_iter()
                            .all(|key| player[key].is_null() || valid_time(&player[key]))
                }))
        { return Err(invalid()); }
    }
    Ok(())
}

fn validate_player_actor_positions(document: &Value) -> Result<(), Error> {
    let capability = &document["capabilities"]["native_player_actor_positions"];
    if capability.is_null() { return Ok(()); }
    let invalid = || Error::new("native Player current positions or their frame associations are incomplete");
    if capability != true { return Err(invalid()); }
    let updates = document["update_frames"].as_array().ok_or_else(invalid)?;
    let draws = document["model_geometry_frames"].as_array().ok_or_else(invalid)?;
    let samples = document["player_actor_position_samples"].as_array().ok_or_else(invalid)?;
    let enabled = document["input"]["players"].as_array().ok_or_else(invalid)?;
    let mut expected = enabled.iter().map(|player| match player["player"].as_str() {
        Some("PlayerNumber_P1") => Ok(0u64),
        Some("PlayerNumber_P2") => Ok(1u64),
        _ => Err(invalid()),
    }).collect::<Result<Vec<_>, _>>()?;
    expected.sort_unstable();
    if expected.is_empty() || expected.len() > 2
        || expected.windows(2).any(|pair| pair[0] == pair[1])
        || samples.is_empty() || samples.len() != draws.len()
    { return Err(invalid()); }
    for (sample, draw) in samples.iter().zip(draws) {
        let at = sample["update_sequence"].as_u64().unwrap_or(0);
        if at == 0 || at > updates.len() as u64 || sample["update_sequence"] != draw["update_sequence"] {
            return Err(invalid());
        }
        let update = &updates[at as usize - 1];
        if sample["scene"] != update["scene"]
            || sample["native_update_frame"] != update["native_update_frame"]
            || sample["native_update_frame"].as_u64().is_none()
        { return Err(invalid()); }
        let players = sample["players"].as_array().ok_or_else(invalid)?;
        let mut observed = Vec::with_capacity(players.len());
        for player in players {
            let index = player["player"].as_u64().ok_or_else(invalid)?;
            if index > 1 || player["name"].as_str() != Some(format!("PlayerP{}", index + 1).as_str())
                || !["x", "y", "z"].into_iter().all(|key| player[key].as_f64()
                    .is_some_and(|value| value.is_finite() && f64::from(value as f32) == value))
            { return Err(invalid()); }
            observed.push(index);
        }
        observed.sort_unstable();
        if observed != expected { return Err(invalid()); }
    }
    Ok(())
}

fn validate_player_current_options(document: &Value) -> Result<(), Error> {
    let capability = &document["capabilities"]["native_player_current_options"];
    if capability.is_null() {
        return Ok(());
    }
    let invalid = || Error::new("native Player Current options or raw perspective getters are incomplete");
    if capability != true || document["capabilities"]["native_player_actor_positions"] != true {
        return Err(invalid());
    }
    validate_player_actor_positions(document)?;
    let native_float = |value: &Value| value.as_f64()
        .is_some_and(|value| value.is_finite() && f64::from(value as f32) == value);
    let approach_speed = |value: &Value| native_float(value)
        && value.as_f64().is_some_and(|speed| speed >= 0.0);
    for sample in document["player_actor_position_samples"].as_array().ok_or_else(invalid)? {
        for player in sample["players"].as_array().ok_or_else(invalid)? {
            let options = &player["current_options"];
            for key in ["perspective_tilt", "skew", "mini", "brake", "drunk", "tipsy"] {
                if !native_float(&options[key]) || !approach_speed(&options["approach_speeds"][key]) {
                    return Err(invalid());
                }
            }
            for method in ["Hallway", "Distant", "Incoming", "Space"] {
                let getter = &options["perspective_getters"][method];
                let valid = match getter["present"].as_bool() {
                    Some(true) => native_float(&getter["value"]) && approach_speed(&getter["approach_speed"]),
                    Some(false) => getter["value"].is_null() && getter["approach_speed"].is_null(),
                    None => false,
                };
                if !valid {
                    return Err(invalid());
                }
            }
        }
    }
    Ok(())
}

fn validate_render_target_passes(document: &Value) -> Result<(), Error> {
    let protocol = &document["capabilities"]["native_render_target_pass_protocol"];
    if protocol.is_null() && document["render_target_passes"].is_null()
        && document["dropped_render_target_passes"].is_null()
    {
        return Ok(());
    }
    let invalid = |detail: &str| Error::new(format!("invalid native render target pass capture: {detail}"));
    if protocol != 1 || document["dropped_render_target_passes"] != 0
        || document["capabilities"]["native_drawable_vertices"] != true
        || !document["input"]["native_source"]["render_target_sha256"].as_str()
            .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(invalid("protocol, dropped events, draw coverage or source provenance"));
    }
    let events = document["render_target_passes"].as_array().ok_or_else(|| invalid("missing pass events"))?;
    let draws = document["drawable_samples"].as_array().ok_or_else(|| invalid("missing submissions"))?;
    let mut open = Vec::new();
    let mut boundary = 0;
    let mut previous_draw = 0;
    let mut previous_update = 0;
    let mut previous_scene = 0;
    for (index, event) in events.iter().enumerate() {
        let cursor = event["after_native_draw_sequence"].as_u64().ok_or_else(|| invalid("missing submission cursor"))?;
        let target = event["target"].as_u64().filter(|target| *target != 0).ok_or_else(|| invalid("missing target handle"))?;
        let parent = event["parent_target"].as_u64().ok_or_else(|| invalid("missing parent handle"))?;
        let name = event["name"].as_str().filter(|name| !name.is_empty()).ok_or_else(|| invalid("missing target name"))?;
        let scene = event["scene"].as_u64().filter(|scene| *scene != 0).ok_or_else(|| invalid("missing scene"))?;
        let update = event["native_update_sequence"].as_u64().ok_or_else(|| invalid("missing native update"))?;
        if cursor < previous_draw || update < previous_update || scene < previous_scene
            || event["sequence"] != index + 1
            || !matches!(event["draw_phase"].as_str(), Some("during_update" | "screen_draw" | "outside_screen"))
            || !(event["actor_draw_sequence"].is_null() || event["actor_draw_sequence"].as_u64().is_some_and(|id| id > 0))
        {
            return Err(invalid("unordered or invalid pass boundary"));
        }
        let active = open.last().map_or(0, |&(target, _, _)| target);
        boundary = validate_pass_draws(draws, boundary, cursor, active)?;
        match event["kind"].as_str() {
            Some("begin") if parent == active && event["active_target"] == target
                && event["preserve"].is_boolean() => open.push((target, name, scene)),
            Some("end") if open.pop() == Some((target, name, scene))
                && parent == open.last().map_or(0, |&(target, _, _)| target)
                && event["active_target"] == parent && event["preserve"].is_null() => (),
            _ => return Err(invalid("unbalanced pass or incorrect active target")),
        }
        previous_draw = cursor;
        previous_update = update;
        previous_scene = scene;
    }
    if !open.is_empty() {
        return Err(invalid("unfinished pass"));
    }
    validate_pass_draws(draws, boundary, u64::MAX, 0)?;
    Ok(())
}

fn validate_pass_draws(draws: &[Value], mut start: usize, through: u64, active: u64) -> Result<usize, Error> {
    while let Some(draw) = draws.get(start) {
        let state = &draw["state"];
        let sequence = state["native_draw_sequence"].as_u64().filter(|sequence| *sequence > 0)
            .ok_or_else(|| Error::new("native render target submission has no sequence"))?;
        if sequence > through {
            break;
        }
        if state["render_target"] != active
            || start > 0 && draws[start - 1]["state"]["native_draw_sequence"].as_u64() >= Some(sequence)
        {
            return Err(Error::new("native submission is out of order or outside its render target pass"));
        }
        start += 1;
    }
    Ok(start)
}

fn validate_interactive_inputs(document: &Value) -> Result<(), Error> {
    validate_fast_note_rendering(document)?;
    validate_song_position_clocks(document)?;
    validate_option_clock_deltas(document)?;
    validate_gameplay_clock_inputs(document)?;
    validate_arrow_update_clocks(document)?;
    validate_player_actor_positions(document)?;
    validate_player_current_options(document)?;
    validate_render_random_inputs(document)?;
    validate_render_target_passes(document)?;
    if document["input"]["notefield_samples"] == true {
        validate_notefield_geometry(document)?;
    }
    if !document["interactive_observer_error"].is_null() {
        return Err(Error::new("native interactive observer failed"));
    }
    let inputs = document["input"]["injected_inputs"] == true;
    if inputs {
        let commands = document["injected_commands"].as_array()
            .ok_or_else(|| Error::new("native injected input delivery was not recorded"))?;
        let script = document["input"]["script"].as_array()
            .ok_or_else(|| Error::new("native injected input script is missing"))?;
        if commands.len() != script.len() || script.is_empty()
            || !commands.iter().zip(script).all(|(actual, expected)| actual["command"] == *expected)
        { return Err(Error::new("native input script was not fully delivered")); }
    }
    if document["capabilities"]["native_input_events"] == true || inputs {
        if document["capabilities"]["native_input_events"] != true || document["dropped_input_events"] != 0 {
            return Err(Error::new("native input-event capture is incomplete"));
        }
        if document["capabilities"]["native_input_step_clocks"] != true {
            return Err(Error::new("native input step clocks are unavailable; rebuild the trace executable"));
        }
        let events = document["input_events"].as_array().ok_or_else(|| Error::new("native input events are missing"))?;
        let frames = document["update_frames"].as_array().ok_or_else(|| Error::new("input events have no frame anchors"))?;
        let mut previous = 0;
        for (index,event) in events.iter().enumerate() {
            let before = event["before_update_sequence"].as_u64().unwrap_or(0);
            if event["sequence"].as_u64() != Some(index as u64+1) || before == 0 || before < previous
                || before > frames.len() as u64
                || !matches!(event["event"]["type"].as_str(), Some("InputEventType_FirstPress"|"InputEventType_Repeat"|"InputEventType_Release"))
            { return Err(Error::new("invalid native input-event anchor or type")); }
            let clocks = event["step_clock_inputs"].as_array()
                .ok_or_else(|| Error::new("native input step clocks are missing"))?;
            if clocks.len() > 2 || (event["event"]["type"] == "InputEventType_Repeat" && !clocks.is_empty())
                || clocks.iter().any(|clock| {
                    clock["player"].as_u64().is_none_or(|n| n > 1)
                        || clock["column"].as_u64().is_none_or(|n| n >= 10)
                        || ["music_seconds", "music_rate", "last_beat_update_ago", "step_ago", "search_seconds"]
                            .iter().any(|key| clock[key].as_f64().is_none_or(|n| !n.is_finite()))
                        || clock["music_rate"].as_f64().is_none_or(|n| n <= 0.0)
                })
            { return Err(Error::new("invalid native input step clocks")); }
            previous = before;
        }
        if inputs {
            for command in document["injected_commands"].as_array().expect("validated commands") {
                let expected = &command["command"];
                let player = format!("PlayerNumber_P{}", expected["player"].as_u64().unwrap_or(0));
                let kind = if expected["down"] == true { "InputEventType_FirstPress" } else { "InputEventType_Release" };
                if !events.iter().any(|event| event["scene"] == expected["scene"]
                    && event["before_update_sequence"].as_u64() >= command["before_update_sequence"].as_u64()
                    && event["event"]["button"] == expected["button"]
                    && event["event"]["PlayerNumber"] == player && event["event"]["type"] == kind)
                { return Err(Error::new("scripted input did not reach native screen input dispatch")); }
            }
        }
    }
    if document["capabilities"]["native_drawable_vertices"] == true {
        if document["dropped_drawable_samples"] != 0 || !document["drawable_samples"].is_array()
            || document["drawable_frames"].as_array().is_none_or(Vec::is_empty)
        { return Err(Error::new("native drawable capture is incomplete")); }
        let samples = document["drawable_samples"].as_array().expect("validated drawable array");
        let frames = document["update_frames"].as_array().ok_or_else(|| Error::new("drawable frame anchors missing"))?;
        let mut previous = 0;
        for sample in samples {
            let at = sample["update_sequence"].as_u64().unwrap_or(0);
            let rows = sample["vertices"].as_array().ok_or_else(|| Error::new("native draw vertices missing"))?;
            let numeric = |v: &Value| v.as_f64().is_some_and(f64::is_finite)
                || v["type"] == "number" && matches!(v["value"].as_str(), Some("nan"|"inf"|"-inf"));
            if sample["state"]["texture"].as_str().is_some_and(|s| !s.is_empty())
                && ["image_width", "image_height", "texture_width", "texture_height"].iter().any(|key|
                    !sample["state"][key].as_f64().is_some_and(|x| x.is_finite() && x > 0.0))
            { return Err(Error::new("native texture allocation dimensions are missing")); }
            if at == 0 || at < previous || at > frames.len() as u64 || rows.is_empty()
                || sample["coordinate_space"] != "native-projection-before-display-centering"
                || sample["scene"] != frames[at as usize-1]["scene"]
                || !rows.iter().all(|row| row.as_array().is_some_and(|row| row.len() == 13
                    && row.iter().all(numeric) && row[7..].iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
                    && (!row[..3].iter().all(Value::is_number) || row[3..7].iter().all(Value::is_number))))
            { return Err(Error::new("invalid native drawable geometry or frame anchor")); }
            previous = at;
        }
        let mut previous = 0;
        let mut consumed = 0;
        for frame in document["drawable_frames"].as_array().expect("validated draw frames") {
            let at = frame["update_sequence"].as_u64().unwrap_or(0);
            let count = frame["samples"].as_u64().unwrap_or(u64::MAX);
            if at == 0 || at < previous || at > frames.len() as u64 || count < consumed || count > samples.len() as u64
                || samples[consumed as usize..count as usize].iter().any(|sample| sample["update_sequence"] != at)
            { return Err(Error::new("native drawable frame boundaries are incomplete")); }
            previous = at; consumed = count;
        }
        if consumed != samples.len() as u64 { return Err(Error::new("native drawable tail was not drawn")); }
    }
    Ok(())
}

fn validate_declaration_forest(document: &Value) -> Result<(), Error> {
    if document.pointer("/capabilities/native_declaration_ancestry").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    let invalid = || Error::new("native actor declaration forest is incomplete or cyclic");
    let definitions = document["actor_definitions"].as_array().ok_or_else(invalid)?;
    let roots = document["roots"].as_array().ok_or_else(invalid)?;
    let mut nodes = std::collections::BTreeMap::new();
    for node in definitions {
        let id = node["id"].as_str().filter(|id| !id.is_empty()).ok_or_else(invalid)?;
        if nodes.insert(id, node).is_some() { return Err(invalid()); }
    }
    for (id, node) in &nodes {
        if let Some(parent) = node.get("parent_id") {
            let owner = parent.as_str().and_then(|parent| nodes.get(parent)).ok_or_else(invalid)?;
            if !owner["children"].as_array().is_some_and(|children| children.iter()
                .any(|child| child["definition_id"].as_str() == Some(*id))) { return Err(invalid()); }
        }
        let children = node["children"].as_array().ok_or_else(invalid)?;
        let mut layers = std::collections::HashSet::with_capacity(children.len());
        for child in children {
            if !child["definition_id"].as_str().is_some_and(|id| nodes.contains_key(id))
                || !child["layer_index"].as_u64().is_some_and(|layer| layer > 0 && layers.insert(layer))
            { return Err(invalid()); }
        }
    }
    let mut root_ids = std::collections::HashSet::with_capacity(roots.len());
    let mut stack = Vec::new();
    for root in roots {
        let id = root.as_str().ok_or_else(invalid)?;
        let node = nodes.get(id).ok_or_else(invalid)?;
        if node.get("parent_id").is_some() || !root_ids.insert(id) { return Err(invalid()); }
        stack.push((id, false));
    }
    let mut active = std::collections::HashSet::new();
    let mut reached = std::collections::HashSet::new();
    while let Some((id, exiting)) = stack.pop() {
        if exiting { active.remove(id); reached.insert(id); continue; }
        if active.contains(id) { return Err(invalid()); }
        if reached.contains(id) { continue; }
        active.insert(id);
        stack.push((id, true));
        // Every referenced ID and children array was validated above.
        for child in nodes[id]["children"].as_array().expect("validated children").iter().rev() {
            stack.push((child["definition_id"].as_str().expect("validated child ID"), false));
        }
    }
    if reached.len() != nodes.len() { return Err(invalid()); }
    Ok(())
}

fn validate_actor_queries(document: &Value) -> Result<(), Error> {
    if document.pointer("/capabilities/native_actor_query_ancestry").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    fn valid(value: &Value) -> bool {
        if let Some(values) = value.as_array() { return values.iter().all(valid); }
        value.get("type").is_some_and(|kind| kind == "nil")
            || value["actor"].as_str().is_some_and(|id| !id.is_empty())
                && value["name"].is_string() && value["path"].is_string()
                && value.get("ancestry_error").is_none()
    }
    let invalid = || Error::new("native actor query ancestry is missing or incomplete");
    for event in document["events"].as_array().ok_or_else(invalid)? {
        if event["kind"] == "query" && matches!(event["operation"].as_str(),
            Some("Actor.GetParent" | "ActorFrame.GetChild" | "ScreenManager.GetTopScreen"))
            && !event.pointer("/detail/results").and_then(Value::as_array)
                .is_some_and(|results| results.len() == 1 && results.iter().all(valid))
        { return Err(invalid()); }
    }
    Ok(())
}

fn validate_actor_identities(document: &Value) -> Result<(), Error> {
    if document.pointer("/capabilities/native_actor_identities").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    let inputs = document["actor_identity_inputs"].as_array()
        .ok_or_else(|| Error::new("native actor identity inputs are missing"))?;
    let events = document["events"].as_array()
        .ok_or_else(|| Error::new("native actor identity inputs have no event anchors"))?;
    let mut actors = std::collections::HashSet::with_capacity(inputs.len());
    for input in inputs {
        let actor = input["actor"].as_str().filter(|id| !id.is_empty());
        let pointer = input["pointer_hex"].as_str().filter(|value| {
            !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && u64::from_str_radix(value, 16).is_ok_and(|pointer| pointer != 0)
        });
        let command = input["command"].as_str();
        let anchor = input["before_seq"].as_u64().and_then(|seq| seq.checked_sub(1))
            .and_then(|index| usize::try_from(index).ok()).and_then(|index| events.get(index));
        if actor.is_none() || pointer.is_none() || !actors.insert(actor)
            || !matches!(command, Some("InitCommand" | "OnCommand"))
            || input["scene"].as_u64().is_none()
            || input["definition_id"].as_str().is_none_or(str::is_empty)
            || input["provenance"] != "native-Luna-tostring-before-authored-command"
            || anchor.is_none_or(|event| event["seq"] != input["before_seq"] || event["operation"] != "command.begin"
                || event["actor"] != input["actor"] || event["command"] != input["command"]
                || event["definition_id"] != input["definition_id"] || event["scene"] != input["scene"])
        {
            return Err(Error::new("invalid native actor identity input or startup anchor"));
        }
    }
    Ok(())
}

fn validate_actor_creations(document: &Value) -> Result<(), Error> {
    if document.pointer("/capabilities/native_actor_creation").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    if !document["actor_creation_error"].is_null() {
        return Err(Error::new("native actor creation observer failed"));
    }
    let inputs = document["actor_creation_inputs"].as_array()
        .ok_or_else(|| Error::new("native actor creation inputs are missing"))?;
    let runtime = document["runtime_actors"].as_array()
        .ok_or_else(|| Error::new("native actor creation inputs have no runtime actors"))?;
    let events = document["events"].as_array()
        .ok_or_else(|| Error::new("native actor creation inputs have no event bounds"))?;
    let mut actors = std::collections::HashSet::with_capacity(inputs.len());
    for (index, input) in inputs.iter().enumerate() {
        let actor = input["actor"].as_str().filter(|id| !id.is_empty());
        let pointer = input["pointer_hex"].as_str().filter(|value| !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            && u64::from_str_radix(value, 16).is_ok_and(|pointer| pointer != 0));
        if actor.is_none() || pointer.is_none() || !actors.insert(actor)
            || input["ordinal"].as_u64() != Some(index as u64 + 1)
            || input["scene"].as_u64().is_none()
            || input["before_seq"].as_u64().is_none_or(|seq| seq == 0 || seq > events.len() as u64 + 1)
            || input["definition_id"].as_str().is_none_or(str::is_empty)
            || input["provenance"] != "native-ActorUtil-before-LoadFromNode"
            || !runtime.iter().any(|record| record["id"] == input["actor"]
                && record["definition_id"] == input["definition_id"])
            || (!input["parent"].is_null() && !runtime.iter().any(|record| record["id"] == input["parent"]))
        {
            return Err(Error::new("invalid native actor creation identity or ownership"));
        }
    }
    if runtime.iter().any(|record| !record["definition_id"].is_null()
        && !actors.contains(&record["id"].as_str()))
    {
        return Err(Error::new("native definition instance has no observed creation identity"));
    }
    Ok(())
}

fn validate_gameplay_preferences(document: &Value) -> Result<(), Error> {
    let preferences = document.pointer("/input/gameplay_preferences")
        .ok_or_else(|| Error::new("native gameplay capture has no initialized gameplay preferences"))?;
    let finite = |key| preferences.get(key).and_then(Value::as_f64).is_some_and(f64::is_finite);
    let windows = preferences.get("timing_windows");
    let players = preferences.get("players").and_then(Value::as_array);
    let valid = document.pointer("/capabilities/native_gameplay_preferences").and_then(Value::as_bool) == Some(true)
        && preferences.get("mod_level").and_then(Value::as_str) == Some("ModsLevel_Preferred")
        && preferences.get("default_modifiers").and_then(Value::as_str).is_some()
        && matches!(preferences.get("initialization").and_then(Value::as_str), Some("theme-game-mode" | "native-preferences"))
        && (preferences.get("initialization").and_then(Value::as_str) != Some("theme-game-mode")
            || preferences.get("game_mode").and_then(Value::as_str).is_some_and(|mode| !mode.is_empty()))
        && ["timing_window_scale", "timing_window_add", "max_input_latency_seconds", "music_rate"].into_iter().all(finite)
        && ["W1", "W2", "W3", "W4", "W5", "Hold", "Mine", "Roll", "Attack", "Checkpoint"].into_iter().all(|key| {
            windows.and_then(|windows| windows.get(key)).and_then(Value::as_f64).is_some_and(f64::is_finite)
        })
        && players.is_some_and(|players| !players.is_empty() && players.len() <= 2 && players.iter().all(|player| {
            matches!(player.get("player").and_then(Value::as_str), Some("PlayerNumber_P1" | "PlayerNumber_P2"))
                && matches!(player.get("fail_setting").and_then(Value::as_str),
                    Some("FailType_Immediate" | "FailType_ImmediateContinue" | "FailType_EndOfSong" | "FailType_Off"))
                && player.get("min_tns_to_hide_notes").and_then(Value::as_str).is_some_and(|score| score.starts_with("TapNoteScore_"))
                && player.get("disabled_timing_windows").and_then(Value::as_array).is_some_and(|windows| windows.iter().all(|window| {
                    matches!(window.as_str(), Some("TimingWindow_W1" | "TimingWindow_W2" | "TimingWindow_W3" | "TimingWindow_W4" | "TimingWindow_W5"))
                }))
        }));
    if !valid {
        return Err(Error::new("native initialized gameplay preferences are incomplete"));
    }
    if document.pointer("/capabilities/native_initial_player_options").and_then(Value::as_bool) == Some(true)
        && !players.is_some_and(|players| players.iter().all(|player| {
            player.get("options_string").and_then(Value::as_str).is_some()
        }))
    {
        return Err(Error::new("native initial player option inputs are incomplete"));
    }
    if document["capabilities"]["native_inactive_player_options"] == true
        && !players.is_some_and(|players| players.len() == 2
            && players.iter().enumerate().all(|(index, player)|
                player["player"] == format!("PlayerNumber_P{}", index + 1)
                    && player["enabled"].is_boolean()
                    && player["options_string"].is_string())
            && players.iter().any(|player| player["enabled"] == true))
    {
        return Err(Error::new("native active/inactive player option inputs are incomplete"));
    }
    Ok(())
}

fn validate_theme_inputs(document: &Value) -> Result<(), Error> {
    if document.pointer("/capabilities/native_initial_theme_modifiers").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    let preferences = &document["input"]["gameplay_preferences"];
    let scalar = |value: &Value| value.is_boolean() || value.is_string()
        || value.as_f64().is_some_and(f64::is_finite);
    let mut names = std::collections::BTreeSet::new();
    let valid = preferences["column_cue_min_time"].as_f64().is_some_and(|time| time.is_finite() && time >= 0.0)
        && preferences["players"].as_array().is_some_and(|players| !players.is_empty() && players.len() <= 2
            && players.iter().all(|player| {
                let name = player["player"].as_str();
                matches!(name, Some("PlayerNumber_P1" | "PlayerNumber_P2")) && names.insert(name)
                    && player["theme_modifiers"].as_object().is_some_and(|modifiers| {
                        modifiers.get("ColumnCues").is_some_and(Value::is_boolean)
                            && modifiers.values().all(|value| scalar(value)
                                || value.as_array().is_some_and(|values| values.iter().all(scalar)))
                    })
            }));
    if !valid {
        return Err(Error::new("native initial theme modifier inputs are incomplete"));
    }
    Ok(())
}

fn config_lua(options: &Options) -> String {
    let inputs = options.inputs.iter().map(|event| format!(
        "{{scene={},seconds={},player={},button={},key={},down={}}}", event.scene,event.seconds,event.player,
        lua_quote(&event.button),lua_quote(&event.key),event.down)).collect::<Vec<_>>().join(",");
    let difficulty = options
        .difficulty
        .as_deref()
        .map(lua_quote)
        .unwrap_or_else(|| "nil".into());
    let base_overlay = format!(
        "/Themes/{}/BGAnimations/ScreenGameplay overlay",
        options.fallback_theme
    );
    format!(
        "ITGMANIA_HARNESS_CONFIG = {{\n\
         \tschema_version = 1,\n\
         \tharness_version = {},\n\
         \tsong = \"Harness/Probe\",\n\
         \tgame = {},\n\
         \tstyle = {},\n\
         \tdifficulty = {},\n\
         \tchart_index = {},\n\
         \tfallback_theme = {},\n\
         \tbase_gameplay_overlay = {},\n\
         \tbase_system_overlay = {},\n\
         \tresult_path = \"/Save/itgmania-harness/song-lua-trace.json\",\n\
         \tuntil_beat = {:.9},\n\
         \tuntil_gameplay_end = {},\n\
         \tautoplay = {},\n\
         \tmax_events = {},\n\
         \tstop_lua_gc = {},\n\
         \trandom_seed = {},\n\
         \tinput_script = {{{}}},\n\
         \tdrawable_samples = {},\n\
         \tnotefield_samples = {},\n\
         \tscene_draws = {},\n\
         \trender_target_passes = {},\n\
         \tdrawable_from_beat = {},\n\
         \tplayer_options = {},\n\
         \tfast_note_rendering = {},\n\
         \tcapture_frame_period_seconds = {},\n\
        }}\n",
        lua_quote(env!("CARGO_PKG_VERSION")),
        lua_quote(&options.game),
        lua_quote(&options.style),
        difficulty,
        options.chart_index.map(|index| index.to_string()).unwrap_or_else(|| "nil".into()),
        lua_quote(&options.fallback_theme),
        lua_quote(&base_overlay),
        lua_quote(&format!("/Themes/{}/BGAnimations/ScreenSystemLayer overlay", options.fallback_theme)),
        options.until_beat,
        options.until_gameplay_end,
        u8::from(options.autoplay),
        options.max_events,
        options.stop_lua_gc,
        options.random_seed,
        inputs,
        options.drawable_samples,
        options.notefield_samples,
        options.scene_draws,
        options.render_target_passes,
        options.drawable_from_beat,
        options.player_options.as_deref().map(lua_quote).unwrap_or_else(|| "nil".into()),
        options.fast_note_rendering.map(|value| value.to_string()).unwrap_or_else(|| "nil".into()),
        options.capture_fps.map(|fps| format!("{:.17}", 1.0 / f64::from(fps))).unwrap_or_else(|| "nil".into())
    )
}

pub(crate) fn lua_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                use fmt::Write;
                let _ = write!(out, "\\{:03}", character as u32);
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

fn create_run_dir() -> Result<PathBuf, Error> {
    let base = std::env::temp_dir().join("itgmania-harness-rs");
    create_dir(&base)?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for attempt in 0..100u32 {
        let path = base.join(format!("song-lua-{}-{nanos}-{attempt}", std::process::id()));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(source) => {
                return Err(Error::new(format!(
                    "could not create runtime directory {}: {source}",
                    path.display()
                )));
            }
        }
    }
    Err(Error::new("could not allocate a unique runtime directory"))
}

fn copy_dir(source: &Path, destination: &Path, recursive: bool, simfile: Option<&Path>) -> Result<(), Error> {
    create_dir(destination)?;
    let entries =
        fs::read_dir(source).map_err(|error| io_error("read directory", source, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("read directory entry", source, error))?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::metadata(&source_path)
            .map_err(|error| io_error("read file metadata", &source_path, error))?;
        if metadata.is_dir() {
            if recursive {
                copy_dir(&source_path, &destination_path, true, None)?;
            }
        } else if metadata.is_file() {
            // NotesLoader.cpp tries SSC before SM and loads a song directory,
            // not the requested filename. A peer chart must not silently
            // replace the capture's declared source. Keep nested Lua/assets.
            if simfile.is_some_and(|selected| source_path != selected)
                && source_path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension|
                    extension.eq_ignore_ascii_case("ssc") || extension.eq_ignore_ascii_case("sm")) {
                continue;
            }
            link_or_copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn link_or_copy(source: &Path, destination: &Path) -> Result<(), Error> {
    if fs::hard_link(source, destination).is_ok() {
        return Ok(());
    }
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| io_error("copy file", source, error))
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    fs::write(path, bytes).map_err(|error| io_error("write file", path, error))
}

fn create_dir(path: &Path) -> Result<(), Error> {
    fs::create_dir_all(path).map_err(|error| io_error("create directory", path, error))
}

fn canonicalize(action: &str, path: &Path) -> Result<PathBuf, Error> {
    path.canonicalize()
        .map_err(|error| io_error(action, path, error))
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> Error {
    Error::new(format!("could not {action} {}: {error}", path.display()))
}

fn slash_path(path: &Path) -> String {
    let path = path.to_string_lossy();
    let path = path
        .strip_prefix("\\\\?\\UNC\\")
        .map(|path| format!("\\\\{path}"))
        .or_else(|| path.strip_prefix("\\\\?\\").map(str::to_owned))
        .unwrap_or_else(|| path.into_owned());
    path.replace('\\', "/")
}

fn log_hint(run_dir: &Path) -> String {
    let log = run_dir.join("Logs/log.txt");
    if log.is_file() {
        format!("; inspect {}", log.display())
    } else {
        String::new()
    }
}

#[derive(Debug)]
pub struct Error {
    message: String,
    run_dir: Option<PathBuf>,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            run_dir: None,
        }
    }

    fn with_run_dir(mut self, run_dir: &Path) -> Self {
        self.run_dir = Some(run_dir.to_owned());
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ITGmania song Lua semantic trace failed: {}",
            self.message
        )?;
        if let Some(run_dir) = &self.run_dir {
            write!(formatter, "; retained run directory {}", run_dir.display())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn whole_song_acceptance_requires_native_script_error_observation() {
        assert!(super::validate_script_errors(&serde_json::json!({"script_errors":[],"dropped_script_errors":0})).is_ok());
        for value in [serde_json::json!({}), serde_json::json!({"script_errors":null}),
            serde_json::json!({"script_errors":[],"dropped_script_errors":1}),
            serde_json::json!({"script_errors":[],"dropped_script_errors":null}),
            serde_json::json!({"script_errors":["FastEnd.lua:47: '<eof>' expected near 'local'"],"dropped_script_errors":0})] {
            assert!(super::validate_script_errors(&value).is_err());
        }
    }
    use super::*;

    fn render_random_fixture() -> Value {
        let mut native = serde_json::json!({
            "capabilities":{"native_render_random_inputs":true,"native_drawable_vertices":true},
            "input":{"native_source":{"actor_sha256":"a".repeat(64),
                "random_numbers_sha256":"b".repeat(64),"random_numbers_header_sha256":"c".repeat(64)},
                "render_random":{"protocol":1,"observation":"native-Actor-PreDraw-vibrate-randomf",
                    "generator":"std::mt19937","distribution":"uniform_real_distribution<double>(-1,1)-to-f32",
                    "seed_source":"native-default-time","association":"active-Actor-Draw-PreDraw-scopes"}},
            "runtime_actors":[{"id":"external-screen","path":"ScreenGameplay"},
                {"id":"def-0001","definition_id":"def-0001","path":"def-0001"}],
            "update_frames":[{"sequence":1,"scene":1}],
            "dropped_render_random_inputs":0,
            "render_random_inputs":[],
            "drawable_samples":[{"scene":1,"update_sequence":1,"render_random_sequences":[1,2,3]},
                {"scene":1,"update_sequence":1,"render_random_sequences":[1,2,4]}],
            "drawable_frames":[{"scene":1,"update_sequence":1,"render_random_inputs":4}]
        });
        for (sequence, owner, native_draw, draw_ordinal, predraw, wrapper) in [
            (1,"external-screen",11,1,1,1), (2,"external-screen",11,1,2,0),
            (3,"def-0001",12,1,1,0), (4,"def-0001",13,2,1,0),
        ] {
            let mut input = serde_json::json!({"sequence":sequence,"scene":1,"update_sequence":1,
                "actor":owner,"path":if owner=="external-screen" {"ScreenGameplay"} else {owner},
                "native_scope_sequence":sequence+20,"native_update_sequence":8,"native_draw_sequence":native_draw,
                "draw_ordinal":draw_ordinal,"predraw_ordinal":predraw,"wrapper_index":wrapper,
                "values":[-0.75,0.25,0.0]});
            if owner == "def-0001" { input["definition_id"] = serde_json::json!(owner); }
            if wrapper == 0 {
                input["source_actor"] = input["actor"].clone();
                input["source_path"] = input["path"].clone();
                if !input["definition_id"].is_null() { input["source_definition_id"] = input["definition_id"].clone(); }
            }
            native["render_random_inputs"].as_array_mut().unwrap().push(input);
        }
        native
    }

    #[test]
    fn render_random_capture_shares_ancestor_scopes_and_distinguishes_repeated_draws() {
        let mut native = render_random_fixture();
        validate_render_random_inputs(&native).unwrap();
        // A repeated owner can have the same ordinal in a later native update
        // before the probe has recorded a new captured frame (immediate Draw).
        native["render_random_inputs"][3]["native_update_sequence"] = serde_json::json!(9);
        native["render_random_inputs"][3]["draw_ordinal"] = serde_json::json!(1);
        validate_render_random_inputs(&native).unwrap();
        // Empty captures still prove that every submitted draw was observed.
        native["render_random_inputs"] = serde_json::json!([]);
        for sample in native["drawable_samples"].as_array_mut().unwrap() {
            sample["render_random_sequences"] = serde_json::json!([]);
        }
        native["drawable_frames"][0]["render_random_inputs"] = serde_json::json!(0);
        validate_render_random_inputs(&native).unwrap();
        validate_render_random_inputs(&serde_json::json!({})).unwrap();
    }

    #[test]
    fn render_random_capture_rejects_missing_or_corrupt_inputs_and_provenance() {
        let native = render_random_fixture();
        for (pointer, value) in [
            ("/capabilities/native_render_random_inputs", serde_json::json!(false)),
            ("/dropped_render_random_inputs", serde_json::json!(1)),
            ("/input/render_random/protocol", serde_json::json!(2)),
            ("/input/render_random/observation", serde_json::json!("derived-from-native-matrix")),
            ("/input/render_random/seed_source", serde_json::json!("lua-random-seed")),
            ("/input/native_source/actor_sha256", Value::Null),
            ("/render_random_inputs/0/sequence", serde_json::json!(2)),
            ("/render_random_inputs/0/actor", serde_json::json!("unknown")),
            ("/render_random_inputs/0/path", serde_json::json!("WrongScreen")),
            ("/render_random_inputs/0/scene", serde_json::json!(2)),
            ("/render_random_inputs/0/update_sequence", serde_json::json!(0)),
            ("/render_random_inputs/0/native_draw_sequence", serde_json::json!(0)),
            ("/render_random_inputs/0/native_update_sequence", serde_json::json!(0)),
            ("/render_random_inputs/0/draw_ordinal", serde_json::json!(0)),
            ("/render_random_inputs/0/predraw_ordinal", serde_json::json!(0)),
            ("/render_random_inputs/0/wrapper_index", serde_json::json!(-2)),
            ("/render_random_inputs/0/values", serde_json::json!([0.25,0.5])),
            ("/render_random_inputs/0/values/1", serde_json::json!(1.25)),
            ("/render_random_inputs/0/values/1", serde_json::json!(0.1)),
            ("/render_random_inputs/1/source_actor", Value::Null),
            ("/render_random_inputs/1/source_actor", serde_json::json!("def-0001")),
            ("/render_random_inputs/1/native_scope_sequence", serde_json::json!(21)),
            ("/render_random_inputs/1/predraw_ordinal", serde_json::json!(1)),
            ("/render_random_inputs/1/wrapper_index", serde_json::json!(1)),
            ("/render_random_inputs/3/draw_ordinal", serde_json::json!(1)),
            ("/drawable_samples/0/render_random_sequences", Value::Null),
            ("/drawable_samples/0/render_random_sequences", serde_json::json!([2,1,3])),
            ("/drawable_samples/0/render_random_sequences", serde_json::json!([1,1,3])),
            ("/drawable_samples/0/render_random_sequences", serde_json::json!([1,2,5])),
            ("/drawable_samples/0/render_random_sequences", serde_json::json!([1,2])),
            ("/drawable_samples/0/scene", serde_json::json!(2)),
            ("/drawable_samples/0/update_sequence", serde_json::json!(2)),
            ("/drawable_frames/0/render_random_inputs", serde_json::json!(3)),
        ] {
            let mut bad = native.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(validate_render_random_inputs(&bad).is_err(), "accepted {pointer}");
        }
        let mut missing_fake_parent = native.clone();
        missing_fake_parent["render_random_inputs"][0]["wrapper_index"] = serde_json::json!(-1);
        assert!(validate_render_random_inputs(&missing_fake_parent).is_err());
    }

    #[test]
    fn interactive_capture_requires_delivered_input_and_complete_draws() {
        let command=serde_json::json!({"scene":1,"seconds":1.0,"player":1,"button":"Right","key":"right","down":true});
        let native=serde_json::json!({"input":{"injected_inputs":true,"script":[command.clone()]},
            "capabilities":{"native_input_events":true,"native_input_step_clocks":true,"native_drawable_vertices":true},
            "update_frames":[{"sequence":1,"scene":1},{"sequence":2,"scene":1}],
            "injected_commands":[{"command":command,"before_update_sequence":1}],
            "input_events":[{"sequence":1,"scene":1,"before_update_sequence":2,"step_clock_inputs":[],
                "event":{"PlayerNumber":"PlayerNumber_P1","button":"Right","type":"InputEventType_FirstPress"}}],
            "dropped_input_events":0,"dropped_drawable_samples":0,
            "drawable_frames":[{"scene":1,"update_sequence":2,"samples":1}],
            "drawable_samples":[{"scene":1,"update_sequence":2,
                "coordinate_space":"native-projection-before-display-centering",
                "vertices":[[0,0,0,-1,1,0,1,0,0,1,1,1,1]]}]});
        validate_interactive_inputs(&native).unwrap();
        for (pointer,value) in [("/input_events",serde_json::json!([])),
            ("/capabilities/native_input_step_clocks",serde_json::json!(false)),
            ("/input_events/0/step_clock_inputs",serde_json::json!(null)),
            ("/drawable_frames/0/samples",serde_json::json!(0)),
            ("/drawable_samples/0/vertices/0/3",serde_json::json!({"type":"number","value":"nan"})),
            ("/input_events/0/before_update_sequence",serde_json::json!(3)),
            ("/dropped_input_events",serde_json::json!(1))] {
            let mut bad=native.clone(); *bad.pointer_mut(pointer).unwrap()=value;
            assert!(validate_interactive_inputs(&bad).is_err(),"accepted {pointer}");
        }
    }

    #[test]
    #[ignore = "requires the native trace executable (ITGMANIA_HARNESS_TRACE_BIN)"]
    fn native_gameplay_clock_inputs_reproduce_stage_observations() {
        let directory = std::env::temp_dir().join(format!("itgmania-gameplay-clock-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let length = 8_000u32 * 6;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + length).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&length.to_le_bytes());
        wav.resize(44 + length as usize, 128);
        fs::write(directory.join("silence.wav"), wav).unwrap();
        fs::write(directory.join("control.sm"), "#TITLE:Gameplay timer inputs;#MUSIC:silence.wav;#BPMS:0=120;#FGCHANGES:0=default.lua=1=0=0=1;\n#NOTES:dance-single:Gameplay timer inputs:Challenge:1:0,0,0,0,0:\n1000\n0000\n0000\n0000\n,\n0000\n0000\n0000\n0000\n,\n0100\n0000\n0000\n0000\n;\n").unwrap();
        fs::write(directory.join("default.lua"), r#"
local pause_started, resumed, changed_rate = nil, false, false
return Def.ActorFrame {
    OnCommand=function(self)
        self:sleep(30)
        self:SetUpdateFunction(function()
            local seconds=GAMESTATE:GetSongPosition():GetMusicSeconds()
            if seconds>=0.5 and not pause_started then
                pause_started=GetTimeSinceStart()
                SCREENMAN:GetTopScreen():PauseGame(true)
            elseif pause_started and not resumed and GetTimeSinceStart()-pause_started>=0.15 then
                SCREENMAN:GetTopScreen():PauseGame(false)
                resumed=true
            end
            if seconds>=1 and not changed_rate then
                GAMESTATE:GetSongOptionsObject('ModsLevel_Song'):MusicRate(1.5)
                GAMESTATE:GetSongOptionsObject('ModsLevel_Current'):MusicRate(1.5)
                changed_rate=true
            end
        end)
    end,
}
"#).unwrap();
        let trace = capture(&Options {
            simfile: directory.join("control.sm"),
            itgmania_bin: std::env::var_os("ITGMANIA_HARNESS_TRACE_BIN").map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target/native-pinned-source/Program/ITGmania-harness-trace.exe")),
            until_beat: 12.0, game: "dance".into(), style: "single".into(),
            difficulty: Some("Challenge".into()), chart_index: None,
            fallback_theme: "Harness Simply Love".into(), timeout: Duration::from_secs(60),
            keep_run_dir: false, headless: true, until_gameplay_end: true, autoplay: true,
            max_events: DEFAULT_MAX_EVENTS, random_seed: 1, inputs: Vec::new(),
            drawable_samples: false, capture_fps: Some(60), fixed_update_fps: Some(30),
            stop_lua_gc: false, notefield_samples: false, scene_draws: false, render_target_passes: false,
            drawable_from_beat: 0.0, player_options: None, fast_note_rendering: None,
        }).unwrap();
        let evidence = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/gameplay-clock-control-native.json");
        fs::create_dir_all(evidence.parent().unwrap()).unwrap();
        fs::write(&evidence, serde_json::to_vec(&trace).unwrap()).unwrap();
        validate_gameplay_clock_inputs(&trace).unwrap();
        let frames = trace["update_frames"].as_array().unwrap();
        let catalog = &trace["input"]["catalog_preferences"];
        let catalog_offset = catalog["global_offset_seconds"].as_f64().unwrap() as f32;
        let catalog_rate = catalog["music_rate"].as_f64().unwrap() as f32;
        let current_offset = trace["input"]["preferences"]["global_offset_seconds"].as_f64().unwrap() as f32;
        let first_second = 0.0_f32 - catalog_rate * catalog_offset;
        let last_second = 4.0_f32 - catalog_rate * catalog_offset;
        let mut before = Vec::new();
        let (mut gameplay, mut steps, mut alive) = (0.0_f32, 0.0_f32, 0.0_f32);
        let (mut reads, mut pause_frames, mut null_run) = (0, 0, 0);
        let (mut resumed_timer, mut changed_rate, mut independent_delta) = (false, false, false);
        for frame in frames {
            before.push((gameplay, steps, alive));
            let clock = &frame["gameplay_clock_inputs"];
            if clock.is_null() {
                if reads > 0 { null_run += 1; }
                continue;
            }
            let delta = clock["delta_seconds"].as_f64().unwrap() as f32;
            let rate = clock["music_rate"].as_f64().unwrap() as f32;
            let beat = frame["song_beat"].as_f64().unwrap() as f32;
            // The control's only notes are beats 0 and 8 at 120 BPM. Song keeps
            // their catalog seconds; its getters apply the current rate/offset.
            let first_beat = (first_second + rate * current_offset) * 2.0;
            let last_beat = (last_second + rate * current_offset) * 2.0;
            let active = beat >= first_beat && beat < last_beat;
            assert_eq!(clock["steps_time_active"], active);
            assert!(rate == 1.0 || rate == 1.5, "unexpected control rate {rate}");
            if null_run >= 5 {
                pause_frames += null_run;
                resumed_timer |= delta >= 0.15;
            }
            null_run = 0;
            changed_rate |= rate == 1.5;
            independent_delta |= f64::from(delta) != frame["delta"].as_f64().unwrap();
            gameplay += delta;
            if active { steps += delta; }
            alive += delta * rate;
            reads += 1;
        }
        assert!(reads > 60 && pause_frames >= 5 && resumed_timer && changed_rate && independent_delta,
            "control did not exercise independent timer, pause and rate change: {reads}/{pause_frames}");
        let mut comparisons = 0;
        for sample in trace["stage_samples"].as_array().unwrap() {
            if sample["phase"] != "progress" { continue; }
            let index = sample["update_sequence"].as_u64().unwrap() as usize - 1;
            let frame = &frames[index];
            assert_eq!(sample["scene"], frame["scene"]);
            assert_eq!(sample["native_update_frame"], frame["native_update_frame"]);
            assert_eq!(sample["native_update_sequence"], frame["native_update_sequence"]);
            let (expected_gameplay, expected_steps, expected_alive) = before[index];
            assert_eq!(sample["gameplay_seconds"].as_f64(), Some(f64::from(expected_gameplay)),
                "gameplay timer phase at update {}", index + 1);
            assert_eq!(sample["steps_seconds"].as_f64(), Some(f64::from(expected_steps)),
                "steps timer phase at update {}", index + 1);
            let player = sample["players"].as_array().unwrap().iter()
                .find(|player| player["player"] == "PlayerNumber_P1").unwrap();
            assert_eq!(player["alive_seconds"].as_f64(), Some(f64::from(expected_alive)),
                "alive timer phase at update {}", index + 1);
            comparisons += 1;
        }
        assert!(comparisons >= 4, "not enough independently observed stage samples");
        println!("gameplay timer control: {reads} reads, {pause_frames} paused frames, {comparisons} exact f32 stage comparisons; {}; fixture={}", evidence.display(), directory.display());
    }

    #[test]
    #[ignore = "requires the native trace executable (ITGMANIA_HARNESS_TRACE_BIN)"]
    fn native_render_random_scopes_preserve_parent_wrapper_and_repeated_draw_inputs() {
        let directory = std::env::temp_dir().join(format!("itgmania-render-random-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let length = 8_000u32 * 4;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + length).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&length.to_le_bytes());
        wav.resize(44 + length as usize, 128);
        fs::write(directory.join("silence.wav"), wav).unwrap();
        fs::write(directory.join("control.sm"), "#TITLE:Render random inputs;#MUSIC:silence.wav;#BPMS:0=120;#FGCHANGES:0=default.lua=1=0=0=1;\n#NOTES:dance-single:Render random inputs:Challenge:1:0,0,0,0,0:\n0000\n1000\n0000\n0000\n;\n").unwrap();
        fs::write(directory.join("default.lua"), r#"
return Def.ActorFrame {
    Name='VibratingParent',
    OnCommand=function(self)
        self:sleep(30)
        SCREENMAN:GetTopScreen():vibrate():effectmagnitude(2,1,0)
        self:vibrate():effectmagnitude(4,3,0)
        self:SetDrawFunction(function(parent)
            parent:GetChild('Repeat'):Draw()
            parent:GetChild('Repeat'):Draw()
            parent:GetChild('FakeChild'):Draw()
        end)
    end,
    Def.Quad { Name='Repeat', InitCommand=function(self)
        self:zoomto(16,16):xy(32,32):vibrate():effectmagnitude(1,2,0)
        self:AddWrapperState():vibrate():effectmagnitude(3,5,0)
    end },
    Def.ActorFrame { Name='Fake', InitCommand=function(self) self:vibrate():effectmagnitude(1,1,0) end },
    Def.Quad { Name='FakeChild', InitCommand=function(self)
        self:zoomto(16,16):xy(80,80):vibrate():effectmagnitude(0,0,0)
    end, OnCommand=function(self) self:SetFakeParent(self:GetParent():GetChild('Fake')) end },
}
"#).unwrap();
        let trace = capture(&Options {
            simfile: directory.join("control.sm"),
            itgmania_bin: std::env::var_os("ITGMANIA_HARNESS_TRACE_BIN").map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target/native-pinned-source/Program/ITGmania-harness-trace.exe")),
            until_beat: 2.0, game: "dance".into(), style: "single".into(),
            difficulty: Some("Challenge".into()), chart_index: None,
            fallback_theme: "Harness Simply Love".into(), timeout: Duration::from_secs(60),
            keep_run_dir: false, headless: true, until_gameplay_end: false, autoplay: true,
            max_events: DEFAULT_MAX_EVENTS, random_seed: 1, inputs: Vec::new(),
            drawable_samples: true, capture_fps: Some(60), fixed_update_fps: None,
            stop_lua_gc: false, notefield_samples: false, scene_draws: false, render_target_passes: false, drawable_from_beat: 0.0, player_options: None, fast_note_rendering: None,
        }).unwrap();
        fs::write(directory.join("native.json"), serde_json::to_vec(&trace).unwrap()).unwrap();
        validate_render_random_inputs(&trace).unwrap();
        let repeat = trace["actor_definitions"].as_array().unwrap().iter()
            .find(|actor| actor["name"] == "Repeat").unwrap()["runtime_actors"][0].as_str().unwrap();
        let draws: Vec<_> = trace["drawable_samples"].as_array().unwrap().iter()
            .filter(|draw| draw["actor"] == repeat).take(2).collect();
        assert_eq!(draws.len(), 2);
        assert_eq!(draws[0]["update_sequence"], draws[1]["update_sequence"]);
        let inputs = trace["render_random_inputs"].as_array().unwrap();
        let scopes = |draw: &Value| -> Vec<&Value> {
            draw["render_random_sequences"].as_array().unwrap().iter()
                .map(|id| &inputs[id.as_u64().unwrap() as usize - 1]).collect()
        };
        let first = scopes(draws[0]);
        let second = scopes(draws[1]);
        assert!(first.len() >= 4, "screen, parent, wrapper and self scopes must be retained");
        for parent in first.iter().filter(|input| input["actor"] != repeat) {
            assert!(second.iter().any(|input| input["sequence"] == parent["sequence"]));
        }
        let first_self = first.iter().copied()
            .find(|input| input["actor"] == repeat && input["wrapper_index"] == 0).unwrap();
        let second_self = second.iter().copied()
            .find(|input| input["actor"] == repeat && input["wrapper_index"] == 0).unwrap();
        assert_ne!(first_self["native_draw_sequence"], second_self["native_draw_sequence"]);
        assert_ne!(first_self["values"], second_self["values"]);
        assert!(first.iter().any(|input| input["actor"] == repeat && input["wrapper_index"] == 1));
        assert!(inputs.iter().any(|input| input["wrapper_index"] == -1
            && input["source_actor"] != input["actor"] && input["source_path"].is_string()));
    }

    #[test]
    #[ignore = "requires the native trace executable (ITGMANIA_HARNESS_TRACE_BIN)"]
    fn native_inputs_are_observed_once_with_or_without_lua_callbacks() {
        let directory = std::env::temp_dir().join(format!("itgmania-input-dispatch-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let length = 8_000u32 * 4;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + length).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&length.to_le_bytes());
        wav.resize(44 + length as usize, 128);
        fs::write(directory.join("silence.wav"), wav).unwrap();
        fs::write(directory.join("control.sm"), "#TITLE:Input dispatch;#MUSIC:silence.wav;#BPMS:0=120;#FGCHANGES:0=default.lua=1=0=0=1;\n#NOTES:dance-single:Input dispatch:Challenge:1:0,0,0,0,0:\n0000\n1000\n0000\n0000\n;\n").unwrap();
        let options = Options {
            simfile: directory.join("control.sm"),
            itgmania_bin: std::env::var_os("ITGMANIA_HARNESS_TRACE_BIN").map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target/native-engine-source/Program/ITGmania-harness-trace.exe")),
            until_beat: 4.0, game: "dance".into(), style: "single".into(),
            difficulty: Some("Challenge".into()), chart_index: None,
            fallback_theme: "Harness Simply Love".into(), timeout: Duration::from_secs(45),
            keep_run_dir: false, headless: true, until_gameplay_end: false, autoplay: false,
            max_events: DEFAULT_MAX_EVENTS, random_seed: 1,
            inputs: [(0.5, true), (0.55, false)].into_iter().map(|(seconds, down)| ScheduledInput {
                scene: 1, seconds, player: 1, button: "Left".into(), key: "left".into(), down,
            }).collect(),
            drawable_samples: false, capture_fps: Some(60), fixed_update_fps: None,
            stop_lua_gc: false, notefield_samples: false, scene_draws: false, render_target_passes: false, drawable_from_beat: 0.0, player_options: None, fast_note_rendering: None,
        };
        for with_callback in [false, true] {
            let callback = if with_callback { r#"
    SCREENMAN:GetTopScreen():AddInputCallback(function(event)
        local stats = STATSMAN:GetCurStageStats():GetPlayerStageStats(PLAYER_1)
        MESSAGEMAN:Broadcast('ObservedCallback', {kind=event.type, button=event.button,
            actual=stats:GetActualDancePoints(), w1=stats:GetTapNoteScores('TapNoteScore_W1')})
        return false
    end)
"# } else { "" };
            let source = r#"
return Def.ActorFrame { OnCommand=function(self)
    self:sleep(30)
    CALLBACK_SETUP
    self:SetUpdateFunction(function(self)
        if GAMESTATE:GetCurMusicSeconds() > 1 then
            local stats = STATSMAN:GetCurStageStats():GetPlayerStageStats(PLAYER_1)
            MESSAGEMAN:Broadcast('InputScore', {actual=stats:GetActualDancePoints(),
                w1=stats:GetTapNoteScores('TapNoteScore_W1'), miss=stats:GetTapNoteScores('TapNoteScore_Miss')})
            self:SetUpdateFunction(nil)
        end
    end)
end }
"#.replace("CALLBACK_SETUP", callback);
            fs::write(directory.join("default.lua"), source).unwrap();
            let trace = capture(&options).unwrap();
            fs::write(directory.join(format!("callback-{with_callback}.json")), serde_json::to_vec(&trace).unwrap()).unwrap();
            let inputs = trace["input_events"].as_array().unwrap();
            assert_eq!(inputs.len(), 2, "one native observation per press/release, callback={with_callback}");
            for (event, kind) in inputs.iter().zip(["InputEventType_FirstPress", "InputEventType_Release"]) {
                assert_eq!(event["event"]["type"], kind);
                assert_eq!(event["event"]["button"], "Left");
                assert_eq!(event["event"]["PlayerNumber"], "PlayerNumber_P1");
                let clocks = event["step_clock_inputs"].as_array().unwrap();
                assert_eq!(clocks.len(), 1);
                assert_eq!(clocks[0]["player"], 0);
                assert_eq!(clocks[0]["column"], 0);
                let number = |key: &str| clocks[0][key].as_f64().unwrap() as f32;
                let step_seconds = (number("music_seconds") + number("last_beat_update_ago") * number("music_rate"))
                    - number("step_ago") * number("music_rate");
                let expected = if kind == "InputEventType_FirstPress" { 0.5 } else { 0.55 };
                assert!((step_seconds - expected).abs() < 0.04, "raw native step clock: {clocks:?}");
            }
            let broadcasts: Vec<_> = trace["events"].as_array().unwrap().iter()
                .filter(|event| event["operation"] == "MessageManager.Broadcast").collect();
            let scores: Vec<_> = broadcasts.iter().filter(|event| event["args"][0] == "InputScore").collect();
            assert_eq!(scores.len(), 1);
            assert_eq!(scores[0]["args"][1], serde_json::json!({"actual":5,"w1":1,"miss":0}),
                "native gameplay must judge the physical input with autoplay disabled");
            let callbacks: Vec<_> = broadcasts.iter().filter(|event| event["args"][0] == "ObservedCallback").collect();
            assert_eq!(callbacks.len(), if with_callback { 2 } else { 0 });
            for (event, kind) in callbacks.iter().zip(["InputEventType_FirstPress", "InputEventType_Release"]) {
                let scored = kind == "InputEventType_Release";
                assert_eq!(event["args"][1], serde_json::json!({"kind":kind,"button":"Left",
                    "actual":if scored {5} else {0},"w1":if scored {1} else {0}}),
                    "Step records the hit before input callbacks; UpdateJudgedRows publishes it next frame");
            }
        }
    }

    #[test]
    #[ignore = "requires the native trace executable (ITGMANIA_HARNESS_TRACE_BIN)"]
    fn native_chart_index_selects_duplicate_difficulties_and_rejects_mismatches() {
        let directory = std::env::temp_dir().join(format!("itgmania-chart-selector-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let length = 8_000u32 * 8;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + length).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&length.to_le_bytes());
        wav.resize(44 + length as usize, 128);
        fs::write(directory.join("silence.wav"), wav).unwrap();
        fs::write(directory.join("default.lua"), r#"
return Def.ActorFrame { OnCommand=function(self)
    self:sleep(30)
    local selected = GAMESTATE:GetCurrentSteps(PLAYER_1)
    MESSAGEMAN:Broadcast('SelectedChart', {description=selected:GetDescription()})
end }
"#).unwrap();
        let mut options = Options {
            simfile: directory.join("control.ssc"),
            itgmania_bin: std::env::var_os("ITGMANIA_HARNESS_TRACE_BIN").map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target/native-engine-source/Program/ITGmania-harness-trace.exe")),
            until_beat: 1.0, game: "dance".into(), style: "single".into(),
            difficulty: Some("Edit".into()), chart_index: Some(2),
            fallback_theme: "Harness Simply Love".into(), timeout: Duration::from_secs(45),
            keep_run_dir: false, headless: true, until_gameplay_end: false, autoplay: true,
            max_events: DEFAULT_MAX_EVENTS, random_seed: 1, inputs: Vec::new(),
            drawable_samples: false, capture_fps: Some(60), fixed_update_fps: None,
            stop_lua_gc: false, notefield_samples: false, scene_draws: false, render_target_passes: false, drawable_from_beat: 0.0, player_options: None, fast_note_rendering: None,
        };
        for extension in ["sm", "ssc"] {
            let mut source = "#VERSION:0.83;#TITLE:Chart selector;#MUSIC:silence.wav;#BPMS:0=120;#FGCHANGES:0=default.lua=1=0=0=1;\n".to_owned();
            for (kind, description, notes) in [
                ("dance-single", "First", "1000\n0000\n0000\n0000"),
                ("dance-single", "Second", "0100\n0000\n0000\n0000"),
                ("pump-single", "Pump", "00100\n00000\n00000\n00000"),
            ] {
                if extension == "sm" {
                    source.push_str(&format!("#NOTES:{kind}:{description}:Edit:1:0,0,0,0,0:\n{notes}\n;\n"));
                } else {
                    source.push_str(&format!("#NOTEDATA:;#STEPSTYPE:{kind};#DESCRIPTION:{description};#CHARTNAME:{description};#DIFFICULTY:Edit;#METER:1;#NOTES:\n{notes}\n;\n"));
                }
            }
            options.simfile = directory.join(format!("control.{extension}"));
            fs::write(&options.simfile, source).unwrap();
            let trace = capture(&options).unwrap();
            fs::write(directory.join(format!("selected-{extension}.json")), serde_json::to_vec(&trace).unwrap()).unwrap();
            assert_eq!(trace["input"]["requested_chart_index"], 2);
            assert_eq!(trace["input"]["starting_chart"]["index"], 2);
            assert_eq!(trace["input"]["starting_chart"]["description"], "Second");
            assert_eq!(trace["input"]["starting_chart"], trace["input"]["chart_catalog"][1]);
            assert!(trace["events"].as_array().unwrap().iter().any(|event|
                event["operation"] == "MessageManager.Broadcast" && event["args"][0] == "SelectedChart"
                    && event["args"][1]["description"] == "Second"),
                "authored code must observe the selected second chart");
        }
        options.chart_index = Some(99999);
        assert!(capture(&options).unwrap_err().to_string().contains("outside Song:GetAllSteps"));
        options.chart_index = Some(3);
        assert!(capture(&options).unwrap_err().to_string().contains("no StepsType_Dance_Single chart"));
        options.chart_index = Some(2);
        options.difficulty = Some("Hard".into());
        assert!(capture(&options).unwrap_err().to_string().contains("matched difficulty Hard"));
        options.difficulty = Some("Edit".into());
        options.game = "pump".into();
        // Simply Love's default Cel skin is dance-only in the pinned install.
        // A direct gameplay capture must declare a valid pump player profile.
        options.player_options = Some("default".into());
        options.chart_index = Some(3);
        let trace = capture(&options).unwrap();
        assert_eq!(trace["input"]["game"], "pump");
        assert_eq!(trace["input"]["starting_chart"]["steps_type"], "StepsType_Pump_Single");
        assert_eq!(trace["input"]["starting_chart"]["description"], "Pump");
    }

    #[test]
    #[ignore = "requires the headless native model geometry adapter"]
    fn native_notefield_capture_includes_models_and_position_caps() {
        let directory =
            std::env::temp_dir().join(format!("itgmania-notefield-control-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let mut wav = Vec::new();
        let length = 8_000u32 * 7;
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + length).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&length.to_le_bytes());
        wav.resize(44 + length as usize, 128);
        fs::write(directory.join("silence.wav"), wav).unwrap();
        // Equal geometry can be fully invisible because the material, rather
        // than the Actor, has zero alpha. Keep that submission as a control.
        for (name, alpha) in [("material", 0.6), ("transparent", 0.0)] {
            fs::write(directory.join(format!("{name}.txt")), format!(r#"// MilkShape 3D ASCII
Frames: 1
Frame: 1
Meshes: 1
"material-control" 0 0
3
0 -16 -16 0 0 0 -1
0 16 -16 0 1 0 -1
0 0 16 0 0.5 1 -1
1
0 0 1
1
0 0 1 2 0 0 0 0
Materials: 1
"control"
0 0 0 1
1 1 1 {alpha}
0 0 0 0
0 0 0 0
0
1
""
""
Bones: 0
"#)).unwrap();
        }
        fs::write(
            directory.join("control.ssc"),
            r#"
    #VERSION:0.83;
    #TITLE:Native notefield capture control;
    #MUSIC:silence.wav;
    #OFFSET:0;
    #BPMS:0=120;
    #FGCHANGES:0=default.lua=1=0=0=1;
    #NOTEDATA:;
    #STEPSTYPE:dance-single;
    #DESCRIPTION:Native geometry control;
    #DIFFICULTY:Challenge;
    #METER:1;
    #NOTES:
    0000
    2000
    0100
    0010
    0001
    3000
    0000
    0000
    ,
    0000
    0000
    2000
    0001
    3000
    0100
    0010
    0000
    ;
    "#,
        )
        .unwrap();
        fs::write(
            directory.join("default.lua"),
            r#"
    return Def.ActorFrame{OnCommand=function(self)
     -- Change only this isolated native control's visual clock. The observer
     -- must read both fields, rather than reconstruct one from initial inputs.
     PREFSMAN:SetPreference('VisualDelaySeconds',0.125)
     self:SetUpdateFunction(function()
      local player=SCREENMAN:GetTopScreen():GetChild('PlayerP1')
      if not player then return end
      for col,actor in ipairs(player:GetChild('NoteField'):GetColumnActors()) do
       local handler=actor:GetPosHandler()
       handler:SetSplineMode('NoteColumnSplineMode_Position')
       handler:SetBeatsPerT(1)
       local spline=handler:GetSpline()
       spline:SetSize(2)
       local direction=GAMESTATE:GetSongBeat()<4 and 1 or -1
       spline:SetPoint(1,{64*(col-2.5),-135*direction,0})
       spline:SetPoint(2,{64*(col-2.5),165*direction,0})
       spline:Solve()
      end
     end)
    end,
    Def.Model{Name='DiffuseGlowControl',Meshes='material.txt',Materials='material.txt',Bones='material.txt',
     InitCommand=function(self)
      self:xy(240,240):diffuse({1,0.5,0.25,0.5}):glow({0.2,0.4,0.6,0.25})
       :blend('BlendMode_Add'):zwrite(false):ztestmode('ZTestMode_Off'):cullmode('CullMode_None')
     end},
    Def.Model{Name='FadingMaterialControl',Meshes='material.txt',Materials='material.txt',Bones='material.txt',
     InitCommand=function(self) self:xy(320,240):diffusealpha(0.9):linear(3):diffusealpha(0.3) end},
    Def.Model{Name='TransparentMaterialControl',Meshes='transparent.txt',Materials='transparent.txt',Bones='transparent.txt',
     InitCommand=function(self) self:xy(400,240) end}}
    "#,
        )
        .unwrap();
        let mut options = Options {
            simfile: directory.join("control.ssc"),
            itgmania_bin: std::env::var_os("ITGMANIA_HARNESS_TRACE_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("target/native-engine-source/Program/ITGmania-harness-trace.exe")
                }),
            until_beat: 6.75,
            game: "dance".into(),
            chart_index: None,
            style: "single".into(),
            difficulty: Some("Challenge".into()),
            fallback_theme: "Harness Simply Love".into(),
            timeout: Duration::from_secs(45),
            keep_run_dir: false,
            headless: true,
            until_gameplay_end: false,
            autoplay: true,
            max_events: DEFAULT_MAX_EVENTS,
            random_seed: 1,
            inputs: Vec::new(),
            drawable_samples: true,
            capture_fps: Some(60),
            fixed_update_fps: Some(60),
            stop_lua_gc: false,
            notefield_samples: true,
            scene_draws: true,
            render_target_passes: false,
            drawable_from_beat: 1.0,
            player_options: Some("m730, 60% mini, cel".into()),
            fast_note_rendering: Some(false),
        };
        let document = capture(&options).unwrap();
        fs::write(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/notefield-control-native.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        assert_eq!(document["input"]["window_visible"], false);
        assert_eq!(document["capabilities"]["native_fast_note_rendering"], true);
        assert_eq!(document["capabilities"]["native_song_position_clocks"], true);
        assert_eq!(document["capabilities"]["native_option_clock_deltas"], true);
        validate_option_clock_deltas(&document).unwrap();
        let clock = document["update_frames"].as_array().unwrap().iter()
            .find(|frame| frame["seconds"].as_f64().unwrap() > 1.0).unwrap();
        assert!((clock["seconds"].as_f64().unwrap() - clock["visible_seconds"].as_f64().unwrap() - 0.125).abs() < 1e-6);
        assert!((clock["song_beat"].as_f64().unwrap() - clock["visible_beat"].as_f64().unwrap() - 0.25).abs() < 1e-5);
        assert_eq!(clock["beat"], clock["visible_beat"]);
        assert_eq!(document["input"]["fast_note_rendering_requested"], false);
        assert_eq!(document["input"]["fast_note_rendering"], false);
        assert_eq!(document["input"]["fast_note_rendering_at_end"], false);
        assert_eq!(document["capabilities"]["native_notefield_geometry"], true);
        assert_eq!(document["capabilities"]["native_model_render_state_protocol"], 1);
        assert_eq!(document["capabilities"]["native_model_render_state_backend"], "opengl");
        assert_eq!(document["capabilities"]["native_receptor_geometry_protocol"], 1);
        let visits = document["receptor_visits"].as_array().unwrap();
        assert!(visits.iter().any(|visit| visit["state"]["base_alpha"] == 1.0),
            "visible receptors must have native draw visits");
        let receptors: Vec<_> = document["drawable_samples"].as_array().unwrap().iter()
            .filter(|sample| sample["state"]["note_context"]["kind"] == "receptor").collect();
        assert!(!receptors.is_empty(), "native receptor sprite geometry must be observed");
        for sample in receptors {
            let sequence = &sample["state"]["note_context"]["receptor_visit_sequence"];
            let visit = visits.iter().find(|visit|
                visit["state"]["note_context"]["receptor_visit_sequence"] == *sequence).unwrap();
            assert_ne!(visit["state"]["base_alpha"], 0.0,
                "native fully transparent receptors must not submit sprites");
        }
        let models = document["model_geometry_samples"].as_array().unwrap();
        let named_model_states = |name: &str| {
            let actor = &document["runtime_actors"].as_array().unwrap().iter()
                .find(|actor| actor["name"] == name).expect("control model is drawn")["id"];
            models.iter().filter(|sample| &sample["actor"] == actor)
                .map(|sample| &sample["state"]["render_state"]).collect::<Vec<_>>()
        };
        let diffuse_glow = named_model_states("DiffuseGlowControl");
        let mut passes = [0; 2];
        for render in diffuse_glow {
            let mode = render["textures"][0]["mode"].as_u64().unwrap() as usize;
            assert!(mode < 2, "native Modulate or Glow pass");
            let expected_alpha = [0.3, 0.25][mode];
            assert!((render["material"]["diffuse"][3].as_f64().unwrap() - expected_alpha).abs() < 1e-6);
            assert!((render["current_color"][3].as_f64().unwrap() - expected_alpha).abs() < 1e-6);
            assert_eq!(render["lighting"], false);
            assert_eq!(render["depth"]["write"], false);
            assert_eq!(render["depth"]["function"], 519); // GL_ALWAYS
            assert_eq!(render["cull"]["enabled"], false);
            assert_eq!(render["blend"]["src_rgb"], 770); // GL_SRC_ALPHA
            // Model resets BLEND_NORMAL after its diffuse mesh loop, before glow.
            assert_eq!(render["blend"]["dst_rgb"], [1, 771][mode]);
            passes[mode] += 1;
        }
        assert!(passes.into_iter().all(|count| count > 20), "diffuse/glow observations: {passes:?}");
        let fading = named_model_states("FadingMaterialControl");
        let alphas: Vec<_> = fading.iter().map(|render| render["material"]["diffuse"][3].as_f64().unwrap()).collect();
        assert!(alphas.iter().copied().fold(f64::NEG_INFINITY, f64::max)
            - alphas.iter().copied().fold(f64::INFINITY, f64::min) > 0.1,
            "native tween must change the submitted material alpha");
        let transparent = named_model_states("TransparentMaterialControl");
        assert!(transparent.len() > 20, "zero-alpha material still submits Model geometry");
        assert!(transparent.iter().all(|render| render["material"]["diffuse"][3] == 0.0
            && render["current_color"][3] == 0.0), "geometry alone cannot establish visibility");
        assert!(
            !models.is_empty(),
            "native Cel note models must reach the recorder"
        );
        assert!(
            models
                .iter()
                .any(|sample| sample["state"]["note_context"]["kind"] == "actor"
                    && sample["state"]["note_context"]["column"].is_u64()),
            "native note identity must accompany model draws"
        );
        let strips: Vec<_> = document["drawable_samples"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|sample| {
                sample["primitive"] == "SymmetricQuadStrip" && sample.get("definition_id").is_none()
            })
            .collect();
        assert!(
            !strips.is_empty(),
            "native hold bodies and caps must survive the declaration filter"
        );
        assert!(
            strips
                .iter()
                .any(|sample| sample["state"]["note_context"]["kind"] == "hold_strip"),
            "native hold part identity"
        );
        let extent = |sample: &&Value| {
            let rows = sample["vertices"].as_array().unwrap();
            let mut low = f64::INFINITY;
            let mut high = f64::NEG_INFINITY;
            for row in rows.iter().skip(1).step_by(3) {
                let y = row[1].as_f64().unwrap();
                low = low.min(y);
                high = high.max(y);
            }
            high - low
        };
        assert!(
            strips
                .iter()
                .any(|sample| extent(sample) > 0.99 && extent(sample) < 1.01),
            "native Position-spline cap rows"
        );
        assert!(
            strips.iter().any(|sample| extent(sample) > 10.0),
            "native hold body rows"
        );
        let mut cap_directions = [false; 2];
        for sample in &strips {
            let rows = sample["vertices"].as_array().unwrap();
            let delta = rows[rows.len() - 2][1].as_f64().unwrap() - rows[1][1].as_f64().unwrap();
            if (delta.abs() - 1.0).abs() < 0.01 {
                cap_directions[usize::from(delta < 0.0)] = true;
            }
        }
        assert_eq!(
            cap_directions,
            [true, true],
            "normal and reversed native caps"
        );
        options.until_beat = 2.0;
        options.notefield_samples = false;
        options.scene_draws = false;
        options.drawable_samples = false;
        let semantic = capture(&options).unwrap();
        assert_eq!(semantic["capabilities"]["native_song_position_clocks"], true);
        let semantic_clock = semantic["update_frames"].as_array().unwrap().iter()
            .find(|frame| frame["seconds"].as_f64().unwrap() > 1.0).unwrap();
        assert_eq!(semantic_clock["beat"], semantic_clock["song_beat"]);
        assert!((semantic_clock["seconds"].as_f64().unwrap() - semantic_clock["visible_seconds"].as_f64().unwrap() - 0.125).abs() < 1e-6);
        assert!((semantic_clock["song_beat"].as_f64().unwrap() - semantic_clock["visible_beat"].as_f64().unwrap() - 0.25).abs() < 1e-5);
        options.notefield_samples = true;
        options.drawable_samples = true;
        options.player_options = Some("m730, 60% mini, cel, 100% dark".into());
        options.fast_note_rendering = Some(true);
        let dark = capture(&options).unwrap();
        assert_eq!(dark["input"]["fast_note_rendering_requested"], true);
        assert_eq!(dark["input"]["fast_note_rendering"], true);
        assert_eq!(dark["input"]["fast_note_rendering_at_end"], true);
        let hidden_visits = dark["receptor_visits"].as_array().unwrap();
        assert!(!hidden_visits.is_empty(), "transparent receptors must still be visited");
        assert!(hidden_visits.iter().all(|visit| visit["state"]["base_alpha"] == 0.0));
        assert!(dark["drawable_samples"].as_array().unwrap().iter().all(|sample|
            sample["state"]["note_context"]["kind"] != "receptor"),
            "fully dark native receptors must submit no sprite geometry");
    }

    #[test]
    fn semantic_captures_require_valid_executable_provenance() {
        let native = serde_json::json!({"input":{"native_source":{
            "revision":"a".repeat(40), "modified_src":false,
            "note_display_sha256":"b".repeat(64), "cubic_spline_sha256":"c".repeat(64)}}});
        validate_native_source(&native).unwrap();
        for field in ["revision", "modified_src", "note_display_sha256", "cubic_spline_sha256"] {
            let mut invalid = native.clone();
            invalid["input"]["native_source"][field] = Value::Null;
            assert!(validate_native_source(&invalid).is_err(), "accepted missing {field}");
        }
        assert!(validate_native_source(&serde_json::json!({})).is_err());
    }

    #[test]
    fn native_model_geometry_requires_complete_draws_and_anchors() {
        let vertex = serde_json::json!([0, 0, 0]);
        let native = serde_json::json!({
            "input":{"notefield_samples":true,"native_source":{
                "revision":"a".repeat(40),"modified_src":false,
                "note_display_sha256":"b".repeat(64),"cubic_spline_sha256":"c".repeat(64),
                "receptor_arrow_row_sha256":"d".repeat(64)}},
            "capabilities":{"native_notefield_geometry":true,"native_model_geometry_protocol":2,"native_receptor_geometry_protocol":1},
            "update_frames":[{"scene":1}],
            "receptor_visits":[],"receptor_frames":[{"update_sequence":1,"samples":0}],
            "dropped_receptor_visits":0,"drawable_samples":[],
            "script_errors":[],"dropped_script_errors":0,
            "dropped_model_geometry_samples":0,
            "model_geometry_frames":[{"update_sequence":1,"samples":1}],
            "model_geometry_meshes":[{"id":1,"vertices":[vertex,vertex,vertex]}],
            "model_geometry_samples":[{"scene":1,"update_sequence":1,"actor":"external-0001",
                "draw_phase":"screen_draw","state":{"render_target":0,"anchors_clip":[[0,0,0,1],[1,0,0,1],[0,1,0,1],[0,0,1,1]]},
                "primitive":"ModelTriangles","geometry":1,
                "coordinate_space":"native-projection-before-display-centering"}]
        });
        validate_interactive_inputs(&native).unwrap();
        for (pointer, value) in [
            ("/input/native_source/revision", serde_json::json!("unknown")),
            ("/script_errors", serde_json::json!([{"message":"authored Lua callback failed"}])),
            ("/dropped_script_errors", serde_json::json!(1)),
            ("/input/native_source/note_display_sha256", serde_json::Value::Null),
            ("/input/native_source/cubic_spline_sha256", serde_json::json!("x".repeat(64))),
            ("/input/native_source/modified_src", serde_json::Value::Null),
            (
                "/capabilities/native_notefield_geometry",
                serde_json::json!(false),
            ),
            ("/dropped_model_geometry_samples", serde_json::json!(1)),
            ("/model_geometry_frames/0/samples", serde_json::json!(0)),
            (
                "/model_geometry_meshes/0/vertices",
                serde_json::json!([[0, 0, 0]]),
            ),
            (
                "/model_geometry_samples/0/update_sequence",
                serde_json::json!(2),
            ),
            ("/model_geometry_samples/0/geometry", serde_json::json!(99)),
            ("/model_geometry_samples/0/state/render_target", serde_json::Value::Null),
            (
                "/model_geometry_samples/0/state/anchors_clip",
                serde_json::Value::Null,
            ),
        ] {
            let mut bad = native.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            assert!(
                validate_interactive_inputs(&bad).is_err(),
                "accepted {pointer}"
            );
        }
    }

    #[test]
    fn native_model_render_state_requires_material_and_actual_renderer_fields() {
        let native = serde_json::json!({
            "capabilities":{"native_model_render_state_protocol":1,"native_model_render_state_backend":"opengl"},
            "model_geometry_samples":[{"state":{"render_state":{
                "backend":"opengl","lighting":false,"current_color":[1,1,1,0.3],
                "material":{"emissive":[0,0,0,0],"ambient":[0.2,0.2,0.2,0.3],
                    "diffuse":[0.8,0.8,0.8,0.3],"specular":[0,0,0,1],"shininess":0},
                "blend":{"enabled":true,"src_rgb":770,"dst_rgb":771,"src_alpha":1,
                    "dst_alpha":771,"equation_rgb":32774,"equation_alpha":32774},
                "depth":{"test_enabled":true,"write":true,"function":515,"range":[0.05,1]},
                "cull":{"enabled":true,"face":1029},
                "alpha_test":{"enabled":true,"function":516,"reference":0},
                "textures":[{"unit":0,"enabled":true,"handle":12,"mode":0,
                    "environment_mode":8448,"texture":"/NoteSkins/dance/cel/parts.png"}]
            }}}]
        });
        validate_model_render_state(&native).unwrap();
        let exported = notefield_render_document(native.clone(), serde_json::json!({}));
        assert_eq!(exported["capabilities"]["native_model_render_state_protocol"], 1);
        assert_eq!(exported["capabilities"]["native_model_render_state_backend"], "opengl");
        assert_eq!(exported["model_geometry_samples"], native["model_geometry_samples"]);
        for path in [
            "/capabilities/native_model_render_state_backend",
            "/model_geometry_samples/0/state/render_state",
            "/model_geometry_samples/0/state/render_state/material/diffuse/3",
            "/model_geometry_samples/0/state/render_state/current_color/3",
            "/model_geometry_samples/0/state/render_state/lighting",
            "/model_geometry_samples/0/state/render_state/blend/src_rgb",
            "/model_geometry_samples/0/state/render_state/depth/write",
            "/model_geometry_samples/0/state/render_state/cull/enabled",
            "/model_geometry_samples/0/state/render_state/alpha_test/reference",
            "/model_geometry_samples/0/state/render_state/textures/0/mode",
            "/model_geometry_samples/0/state/render_state/textures/0/texture",
        ] {
            let mut missing = native.clone();
            *missing.pointer_mut(path).unwrap() = Value::Null;
            assert!(validate_model_render_state(&missing).is_err(), "accepted missing {path}");
        }
        let mut transparent = native.clone();
        transparent["model_geometry_samples"][0]["state"]["render_state"]["material"]["diffuse"][3] = 0.into();
        transparent["model_geometry_samples"][0]["state"]["render_state"]["current_color"][3] = 0.into();
        validate_model_render_state(&transparent).unwrap();
        assert_ne!(transparent, native, "transparent submission must retain distinct observed alpha");
        validate_model_render_state(&serde_json::json!({})).unwrap();
    }

    #[test]
    fn native_receptor_capture_requires_visits_even_when_no_sprites_are_drawn() {
        let context = serde_json::json!({"kind":"receptor","player":0,"column":0,
            "draw_beat":1.5,"note_beat":1.5,"part":-1,"held":false,"receptor_visit_sequence":7});
        let mut native = serde_json::json!({
            "input":{"native_source":{"receptor_arrow_row_sha256":"a".repeat(64)}},
            "capabilities":{"native_receptor_geometry_protocol":1},
            "update_frames":[{"scene":1}],"model_geometry_frames":[{"update_sequence":1,"samples":0}],
            "model_geometry_samples":[],"drawable_samples":[],"dropped_receptor_visits":0,
            "receptor_frames":[{"update_sequence":1,"samples":1}],
            "receptor_visits":[{"scene":1,"update_sequence":1,"actor":"native-receptor-row",
                "state":{"base_alpha":0.0,"visible":true,"render_target":0,"draw_phase":"screen_draw","note_context":context}}]
        });
        validate_receptor_geometry(&native).unwrap();
        for (pointer, value) in [
            ("/capabilities/native_receptor_geometry_protocol", serde_json::json!(0)),
            ("/input/native_source/receptor_arrow_row_sha256", Value::Null),
            ("/dropped_receptor_visits", serde_json::json!(1)),
            ("/receptor_frames/0/samples", serde_json::json!(0)),
            ("/receptor_visits/0/update_sequence", serde_json::json!(2)),
            ("/receptor_visits/0/state/base_alpha", Value::Null),
            ("/receptor_visits/0/state/visible", Value::Null),
        ] {
            let mut invalid = native.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(validate_receptor_geometry(&invalid).is_err(), "accepted {pointer}");
        }
        native["receptor_visits"][0]["state"]["base_alpha"] = serde_json::json!(1.0);
        native["drawable_samples"] = serde_json::json!([{"scene":1,"update_sequence":1,
            "state":{"render_target":0,"note_context":context}}]);
        validate_receptor_geometry(&native).unwrap();
        native["drawable_samples"][0]["state"]["note_context"]["receptor_visit_sequence"] = serde_json::json!(8);
        assert!(validate_receptor_geometry(&native).is_err(), "unobserved receptor draw was accepted");
    }

    #[test]
    fn native_scene_geometry_requires_one_submission_order_across_channels() {
        let mut native = serde_json::json!({
            "input":{"scene_draws":true,"native_source":{"receptor_arrow_row_sha256":"a".repeat(64)}},
            "capabilities":{"native_receptor_geometry_protocol":1},
            "update_frames":[{"scene":1}],"model_geometry_frames":[{"update_sequence":1,"samples":1}],
            "model_geometry_samples":[{"state":{"native_draw_sequence":2}}],
            "drawable_samples":[{"state":{"native_draw_sequence":1}},{"state":{"native_draw_sequence":3}}],
            "dropped_receptor_visits":0,"receptor_frames":[{"update_sequence":1,"samples":0}],"receptor_visits":[]
        });
        validate_receptor_geometry(&native).unwrap();
        for bad in [Value::Null, serde_json::json!(0), serde_json::json!(1), serde_json::json!(3)] {
            native["model_geometry_samples"][0]["state"]["native_draw_sequence"] = bad;
            assert!(validate_receptor_geometry(&native).is_err(), "missing or reused geometry order accepted");
        }
        native["model_geometry_samples"][0]["state"]["native_draw_sequence"] = serde_json::json!(2);
        native["drawable_samples"][1]["state"]["native_draw_sequence"] = serde_json::json!(0);
        assert!(validate_receptor_geometry(&native).is_err(), "reordered channel accepted");
    }

    #[test]
    fn lua_strings_escape_paths_and_control_characters() {
        assert_eq!(
            lua_quote("C:\\Songs\\a\"b\n"),
            "\"C:\\\\Songs\\\\a\\\"b\\n\""
        );
    }

    #[test]
    fn external_paths_do_not_leak_windows_verbatim_prefixes() {
        assert_eq!(
            slash_path(Path::new("\\\\?\\C:\\Games\\ITGmania")),
            "C:/Games/ITGmania"
        );
        assert_eq!(
            slash_path(Path::new("\\\\?\\UNC\\server\\share\\ITGmania")),
            "//server/share/ITGmania"
        );
    }

    #[test]
    fn generated_config_has_stable_runtime_contract() {
        let mut options = Options {
            simfile: PathBuf::from("song.ssc"),
            itgmania_bin: PathBuf::from("ITGmania.exe"),
            until_beat: 4.5,
            game: "dance".into(),
            chart_index: None,
            style: "single".into(),
            difficulty: Some("Challenge".into()),
            fallback_theme: "Simply-Love-SM5".into(),
            timeout: Duration::from_secs(30),
            keep_run_dir: false,
            headless: false,
            until_gameplay_end: false,
            autoplay: true,
            max_events: DEFAULT_MAX_EVENTS,
            random_seed: 73,
            inputs: Vec::new(),
            drawable_samples: false,
            capture_fps: None,
            fixed_update_fps: None,
            stop_lua_gc: false,
            notefield_samples: false,
            scene_draws: false,
            render_target_passes: false,
            drawable_from_beat: 0.0,
            player_options: None,
            fast_note_rendering: None,
        };
        let lua = config_lua(&options);
        assert!(lua.contains("schema_version = 1"));
        assert!(lua.contains("until_beat = 4.500000000"));
        assert!(lua.contains("difficulty = \"Challenge\""));
        assert!(lua.contains("max_events = 250000"));
        assert!(lua.contains("random_seed = 73"));
        assert!(lua.contains("game = \"dance\""));
        assert!(lua.contains("chart_index = nil"));
        assert!(lua.contains("scene_draws = false"));
        assert!(lua.contains("fast_note_rendering = nil"));
        options.fast_note_rendering = Some(true);
        assert!(config_lua(&options).contains("fast_note_rendering = true"));
        options.fast_note_rendering = Some(false);
        assert!(config_lua(&options).contains("fast_note_rendering = false"));
        options.scene_draws = true;
        assert!(validate_options(&options).is_err(), "scene draws require a geometry profile");
        options.game = "pump".into();
        options.chart_index = Some(7);
        assert!(config_lua(&options).contains("game = \"pump\""));
        assert!(config_lua(&options).contains("chart_index = 7"));
        options.chart_index = Some(0);
        assert!(validate_options(&options).is_err());
        options.chart_index = None;
        options.game = String::new();
        assert!(validate_options(&options).is_err());
        options.game = "dance".into();
        options.notefield_samples=true; options.drawable_samples=true; options.headless=true;
        options.drawable_from_beat=2.0;
        options.player_options=Some("m730, 60% mini, cel".into());
        let profiled = config_lua(&options);
        assert!(profiled.contains("notefield_samples = true"));
        assert!(profiled.contains("scene_draws = true"));
        assert!(profiled.contains("drawable_from_beat = 2"));
        assert!(profiled.contains("player_options = \"m730, 60% mini, cel\""));
        for seed in [0, u32::MAX] {
            options.random_seed = seed;
            assert!(validate_options(&options).is_err());
        }
        options.random_seed = 1;
        options.max_events = 2_000_000;
        assert!(validate_options(&options).is_ok());
        assert!(config_lua(&options).contains("max_events = 2000000"));
        options.max_events = 0;
        assert!(validate_options(&options).is_err());
    }

    #[test]
    fn update_clocks_allow_reload_music_reset() {
        let document = serde_json::json!({
            "capabilities": {"native_update_frames": true, "native_machine_preferences": true,
                "native_gameplay_preferences": true},
            "input": {"preferences": {"global_offset_seconds": -0.008,
                "default_sync_offset": "SyncOffset_ITG", "visual_delay_seconds": 0},
                "gameplay_preferences": {"initialization": "theme-game-mode", "game_mode": "ITG",
                    "mod_level": "ModsLevel_Preferred", "default_modifiers": "failendofsong", "timing_window_scale": 1,
                    "timing_window_add": 0.0015, "max_input_latency_seconds": 0, "music_rate": 1,
                    "timing_windows": {"W1": 0.0215, "W2": 0.043, "W3": 0.102,
                        "W4": 0.135, "W5": 0.18, "Hold": 0.32, "Mine": 0.07,
                        "Roll": 0.35, "Attack": 0.135, "Checkpoint": 0.1664},
                    "players": [{"player": "PlayerNumber_P1", "fail_setting": "FailType_ImmediateContinue",
                        "min_tns_to_hide_notes": "TapNoteScore_W3", "disabled_timing_windows": []}]}},
            "dropped_update_frames": 0,
            "update_frame_phase": "gameplay-overlay-probe",
            "update_frames": [
                {"sequence": 1, "scene": 1, "delta": 0, "seconds": -1, "beat": -2, "wall_seconds": 10},
                {"sequence": 2, "scene": 1, "delta": 0.02, "seconds": 1, "beat": 2, "wall_seconds": 10.02},
                {"sequence": 3, "scene": 2, "delta": 0, "seconds": -6, "beat": -6, "wall_seconds": 10.03}
            ]
        });
        assert!(validate_runtime_inputs(&document).is_ok());
        let mut observed = document.clone();
        observed["script_errors"] = serde_json::json!([]);
        observed["dropped_script_errors"] = serde_json::json!(0);
        assert!(validate_runtime_inputs(&observed).is_ok());
        for (errors, dropped) in [
            (serde_json::json!(["invalid Lua source"]), serde_json::json!(0)),
            (serde_json::json!([]), serde_json::json!(1)),
            (Value::Null, serde_json::json!(0)),
            (serde_json::json!([]), Value::Null),
        ] {
            observed["script_errors"] = errors;
            observed["dropped_script_errors"] = dropped;
            assert!(validate_runtime_inputs(&observed).is_err());
        }
        let mut seeded = document.clone();
        seeded["capabilities"]["native_random_seed"] = serde_json::json!(true);
        assert!(validate_runtime_inputs(&seeded).is_err());
        seeded["random_seed"] = serde_json::json!(73);
        seeded["random_reseeds"] = serde_json::json!([]);
        seeded["dropped_random_reseeds"] = serde_json::json!(0);
        assert!(validate_runtime_inputs(&seeded).is_ok());
        for seed in [serde_json::json!(0), serde_json::json!(2147483648_u64), Value::Null] {
            seeded["random_seed"] = seed;
            assert!(validate_runtime_inputs(&seeded).is_err());
        }
        seeded["random_seed"] = serde_json::json!(1);
        seeded["dropped_random_reseeds"] = serde_json::json!(1);
        assert!(validate_runtime_inputs(&seeded).is_err());
        let mut recorded = document.clone();
        recorded["capabilities"]["native_display_preference"] = serde_json::json!(true);
        recorded["capabilities"]["native_initial_player_options"] = serde_json::json!(true);
        assert!(validate_runtime_inputs(&recorded).is_err());
        recorded["input"]["preferences"]["display_aspect_ratio"] = serde_json::json!(16.0_f32 / 9.0);
        assert!(validate_runtime_inputs(&recorded).is_err());
        recorded["input"]["gameplay_preferences"]["players"][0]["options_string"] =
            serde_json::json!("NoHideLights, PlayerAutoPlay, Overhead");
        assert!(validate_runtime_inputs(&recorded).is_ok());
        let mut both = recorded.clone();
        both["capabilities"]["native_inactive_player_options"] = serde_json::json!(true);
        assert!(validate_runtime_inputs(&both).is_err());
        let mut second = both["input"]["gameplay_preferences"]["players"][0].clone();
        second["player"] = serde_json::json!("PlayerNumber_P2");
        second["enabled"] = serde_json::json!(false);
        both["input"]["gameplay_preferences"]["players"][0]["enabled"] = serde_json::json!(true);
        both["input"]["gameplay_preferences"]["players"].as_array_mut().unwrap().push(second);
        assert!(validate_runtime_inputs(&both).is_ok());
        for (path, value) in [
            ("/input/gameplay_preferences/players/1/enabled", Value::Null),
            ("/input/gameplay_preferences/players/1/player", serde_json::json!("PlayerNumber_P1")),
            ("/input/gameplay_preferences/players/1/options_string", Value::Null),
            ("/input/gameplay_preferences/players/0/enabled", serde_json::json!(false)),
        ] {
            let mut invalid = both.clone();
            *invalid.pointer_mut(path).unwrap() = value;
            assert!(validate_runtime_inputs(&invalid).is_err(), "accepted incomplete roster: {path}");
        }
        recorded["input"]["preferences"]["display_aspect_ratio"] = serde_json::json!(-1);
        assert!(validate_runtime_inputs(&recorded).is_err());
        let mut lost = document.clone();
        lost["dropped_update_frames"] = serde_json::json!(1);
        assert!(validate_runtime_inputs(&lost).is_err());
        let mut reversed = document.clone();
        reversed["update_frames"][2]["wall_seconds"] = serde_json::json!(9);
        assert!(validate_runtime_inputs(&reversed).is_err());
        let mut missing = document.clone();
        missing["update_frames"][1]["delta"] = Value::Null;
        assert!(validate_runtime_inputs(&missing).is_err());
        let mut gap = document.clone();
        gap["update_frames"][2]["sequence"] = serde_json::json!(4);
        assert!(validate_runtime_inputs(&gap).is_err());
        let mut settings = document.clone();
        settings["input"]["preferences"]["global_offset_seconds"] = Value::Null;
        assert!(validate_runtime_inputs(&settings).is_err());
        settings = document.clone();
        settings["input"]["preferences"]["default_sync_offset"] = serde_json::json!("unknown");
        assert!(validate_runtime_inputs(&settings).is_err());
        settings = document.clone();
        settings["capabilities"]["native_machine_preferences"] = serde_json::json!(false);
        assert!(validate_runtime_inputs(&settings).is_err());
        for pointer in ["/input/gameplay_preferences", "/input/gameplay_preferences/timing_windows/Roll",
            "/input/gameplay_preferences/timing_window_add", "/input/gameplay_preferences/game_mode",
            "/input/gameplay_preferences/players/0/fail_setting", "/capabilities/native_gameplay_preferences"] {
            settings = document.clone();
            *settings.pointer_mut(pointer).expect("fixture contains preference") = Value::Null;
            assert!(validate_runtime_inputs(&settings).is_err(), "accepted missing {pointer}");
        }
        settings = document.clone();
        settings["input"]["gameplay_preferences"]["initialization"] = serde_json::json!("native-preferences");
        settings["input"]["gameplay_preferences"]["game_mode"] = Value::Null;
        assert!(validate_runtime_inputs(&settings).is_ok());

        let mut epochs = document.clone();
        epochs["capabilities"]["native_event_scenes"] = serde_json::json!(true);
        epochs["events"] = serde_json::json!([
            {"seq": 1, "scene": 0}, {"seq": 2, "scene": 1}, {"seq": 3, "scene": 2}
        ]);
        assert!(validate_runtime_inputs(&epochs).is_ok());
        for (pointer, value) in [
            ("/events/2/scene", Value::Null),
            ("/events/2/scene", serde_json::json!(3)),
            ("/events/2/scene", serde_json::json!(0)),
            ("/events/2/seq", serde_json::json!(4)),
            ("/events", Value::Null),
        ] {
            let mut invalid = epochs.clone();
            *invalid.pointer_mut(pointer).expect("native event field") = value;
            assert!(validate_runtime_inputs(&invalid).is_err(), "accepted invalid event at {pointer}");
        }

        epochs["capabilities"]["native_call_sources"] = serde_json::json!(true);
        epochs["events"][1]["kind"] = serde_json::json!("call");
        epochs["events"][1]["source"] = serde_json::json!("song:/reload.lua");
        epochs["events"][1]["caller"] = serde_json::json!({"source":"@Themes/Simply Love/overlay.lua", "line":12});
        epochs["events"][2]["kind"] = serde_json::json!("query");
        epochs["events"][2]["caller"] = serde_json::json!({"source":"song:/reload.lua", "line":24});
        assert!(validate_runtime_inputs(&epochs).is_ok());
        let mut metric_command = epochs.clone();
        metric_command["events"][1]["caller"] = serde_json::json!({"source":"", "line":1});
        assert!(validate_runtime_inputs(&metric_command).is_ok(), "native metric commands have an empty Lua chunk name");
        for (pointer, value) in [
            ("/events/1/caller", Value::Null),
            ("/events/1/caller/source", Value::Null),
            ("/events/2/caller/line", serde_json::json!(-2)),
            ("/events/2/caller/line", serde_json::json!(1.5)),
            ("/events", Value::Null),
        ] {
            let mut invalid = epochs.clone();
            *invalid.pointer_mut(pointer).expect("native caller field") = value;
            assert!(validate_runtime_inputs(&invalid).is_err(), "accepted invalid caller at {pointer}");
        }
        epochs["capabilities"]["native_constructor_queries"] = serde_json::json!(true);
        epochs["events"][1]["phase"] = serde_json::json!("callback");
        epochs["events"][2]["phase"] = serde_json::json!("construction");
        epochs["events"][2]["detail"] = serde_json::json!({"results":[{"type":"Steps","index":2}]});
        assert!(validate_runtime_inputs(&epochs).is_ok());
        for (pointer, value) in [
            ("/capabilities/native_call_sources", serde_json::json!(false)),
            ("/events/1/phase", Value::Null),
            ("/events/2/phase", serde_json::json!("unknown")),
            ("/events/2/detail/results", Value::Null),
            ("/events/2/detail/results", serde_json::json!({})),
        ] {
            let mut invalid = epochs.clone();
            *invalid.pointer_mut(pointer).expect("native query field") = value;
            assert!(validate_runtime_inputs(&invalid).is_err(), "accepted incomplete query at {pointer}");
        }
        epochs["capabilities"]["native_query_classes"] = serde_json::json!(["Song", "Steps", "PrefsManager", "RageDisplay", "Style"]);
        assert!(validate_runtime_inputs(&epochs).is_ok());
        for value in [serde_json::json!([]), serde_json::json!(["Song", "Song"]),
            serde_json::json!([""]), serde_json::json!([false]), serde_json::json!({})] {
            let mut invalid = epochs.clone();
            invalid["capabilities"]["native_query_classes"] = value;
            assert!(validate_runtime_inputs(&invalid).is_err(), "accepted invalid query class inventory");
        }
        epochs["capabilities"]["native_modifier_results"] = serde_json::json!(true);
        epochs["events"][1]["kind"] = serde_json::json!("modifier");
        epochs["events"][1]["detail"] = serde_json::json!({"results":[0.25, 1.0]});
        assert!(validate_runtime_inputs(&epochs).is_ok());
        for result in [Value::Null, serde_json::json!({})] {
            let mut invalid = epochs.clone();
            invalid["events"][1]["detail"]["results"] = result;
            assert!(validate_runtime_inputs(&invalid).is_err(), "accepted missing modifier returns");
        }
    }

    #[test]
    fn declaration_forest_requires_closed_ancestry() {
        let document = serde_json::json!({
            "capabilities": {"native_declaration_ancestry": true}, "roots": ["root"],
            "actor_definitions": [
                {"id":"root", "children":[{"layer_index":1,"definition_id":"child"}],"runtime_actors":[]},
                {"id":"child", "parent_id":"root", "children":[],"runtime_actors":["child"]},
            ],
        });
        assert!(validate_declaration_forest(&document).is_ok());
        assert!(validate_declaration_forest(&serde_json::json!({})).is_ok());
        for (pointer, value) in [
            ("/roots", Value::Null), ("/roots", serde_json::json!([])),
            ("/roots/0", serde_json::json!("absent")),
            ("/actor_definitions/1/parent_id", serde_json::json!("absent")),
            ("/actor_definitions/1/parent_id", serde_json::json!("child")),
            ("/actor_definitions/1/id", serde_json::json!("root")),
            ("/actor_definitions/0/children/0/definition_id", serde_json::json!("absent")),
            ("/actor_definitions/0/children/0/layer_index", serde_json::json!(0)),
            ("/actor_definitions/1/children", serde_json::json!([{"layer_index":1,"definition_id":"root"}])),
        ] {
            let mut invalid = document.clone();
            *invalid.pointer_mut(pointer).expect("declaration field") = value;
            assert!(validate_declaration_forest(&invalid).is_err(), "accepted invalid {pointer}");
        }
        let mut unused = document.clone();
        unused["actor_definitions"].as_array_mut().expect("definition array")
            .push(serde_json::json!({"id":"unused","children":[]}));
        assert!(validate_declaration_forest(&unused).is_err());
        let mut shared = document.clone();
        shared["roots"].as_array_mut().expect("roots").push(serde_json::json!("other"));
        shared["actor_definitions"].as_array_mut().expect("definition array")
            .push(serde_json::json!({"id":"other","children":[{"layer_index":1,"definition_id":"child"}]}));
        assert!(validate_declaration_forest(&shared).is_ok());
    }

    #[test]
    fn actor_queries_require_observed_ancestry() {
        let document = serde_json::json!({
            "capabilities":{"native_actor_query_ancestry":true},
            "events":[{"kind":"query","operation":"Actor.GetParent",
                "detail":{"results":[{"actor":"parent","name":"","path":"ScreenGameplay"}]}},
                {"kind":"query","operation":"Actor.GetParent","detail":{"results":[{"type":"nil"}]}}],
        });
        assert!(validate_actor_queries(&document).is_ok());
        assert!(validate_actor_queries(&serde_json::json!({})).is_ok());
        let mut duplicates = document.clone();
        duplicates["events"][0]["operation"] = serde_json::json!("ActorFrame.GetChild");
        duplicates["events"][0]["detail"]["results"] = serde_json::json!([[
            {"actor":"first","name":"Twin","path":"ScreenGameplay/Twin"},
            {"actor":"second","name":"Twin","path":"ScreenGameplay/Twin"},
        ]]);
        assert!(validate_actor_queries(&duplicates).is_ok());
        for (pointer, value) in [
            ("/events", Value::Null), ("/events/0/detail/results", serde_json::json!([])),
            ("/events/0/detail/results/0/path", Value::Null),
            ("/events/0/detail/results/0/name", Value::Null),
            ("/events/0/detail/results/0/actor", serde_json::json!("")),
            ("/events/0/detail/results/0/ancestry_error", serde_json::json!("cycle")),
        ] {
            let mut invalid = document.clone();
            if pointer.ends_with("ancestry_error") {
                invalid["events"][0]["detail"]["results"][0]["ancestry_error"] = value;
            } else { *invalid.pointer_mut(pointer).expect("query field") = value; }
            assert!(validate_actor_queries(&invalid).is_err(), "accepted invalid {pointer}");
        }
    }

    #[test]
    fn actor_identities_require_startup_anchors() {
        let document = serde_json::json!({
            "capabilities": {"native_actor_identities": true},
            "events": [{"seq": 1, "scene": 0, "operation": "command.begin", "actor": "def-0001",
                "definition_id": "def-0001", "command": "InitCommand"}],
            "actor_identity_inputs": [{"actor": "def-0001", "definition_id": "def-0001",
                "pointer_hex": "abcdef123456", "scene": 0, "before_seq": 1, "command": "InitCommand",
                "provenance": "native-Luna-tostring-before-authored-command"}],
        });
        assert!(validate_actor_identities(&document).is_ok());
        assert!(validate_actor_identities(&serde_json::json!({})).is_ok());
        for (pointer, value) in [
            ("/actor_identity_inputs", Value::Null),
            ("/actor_identity_inputs/0/pointer_hex", serde_json::json!("0")),
            ("/actor_identity_inputs/0/pointer_hex", serde_json::json!("0xabcdef")),
            ("/actor_identity_inputs/0/pointer_hex", serde_json::json!("10000000000000000")),
            ("/actor_identity_inputs/0/before_seq", serde_json::json!(0)),
            ("/actor_identity_inputs/0/scene", serde_json::json!(1)),
            ("/actor_identity_inputs/0/definition_id", serde_json::json!("def-0002")),
            ("/actor_identity_inputs/0/provenance", serde_json::json!("observed-callback-order")),
            ("/events/0/actor", serde_json::json!("def-0002")),
            ("/events/0/seq", serde_json::json!(2)),
            ("/events/0/operation", serde_json::json!("command.end")),
        ] {
            let mut invalid = document.clone();
            *invalid.pointer_mut(pointer).expect("native input field") = value;
            assert!(validate_actor_identities(&invalid).is_err(), "accepted invalid {pointer}");
        }
        let mut message = document.clone();
        message["actor_identity_inputs"][0]["command"] = serde_json::json!("GoMessageCommand");
        message["events"][0]["command"] = serde_json::json!("GoMessageCommand");
        assert!(validate_actor_identities(&message).is_err());
        let mut duplicate = document.clone();
        duplicate["actor_identity_inputs"].as_array_mut().expect("input array")
            .push(document["actor_identity_inputs"][0].clone());
        assert!(validate_actor_identities(&duplicate).is_err());
    }

    #[test]
    fn actor_creations_require_observed_instances() {
        let document = serde_json::json!({
            "capabilities": {"native_actor_creation": true}, "events": [],
            "runtime_actors": [{"id": "root", "definition_id": "def-1"},
                {"id": "child", "definition_id": "def-2"}],
            "actor_creation_inputs": [
                {"actor": "root", "definition_id": "def-1", "pointer_hex": "abcd",
                    "scene": 0, "before_seq": 1, "ordinal": 1,
                    "provenance": "native-ActorUtil-before-LoadFromNode"},
                {"actor": "child", "definition_id": "def-2", "parent": "root", "pointer_hex": "abce",
                    "scene": 0, "before_seq": 1, "ordinal": 2,
                    "provenance": "native-ActorUtil-before-LoadFromNode"}]
        });
        assert!(validate_actor_creations(&document).is_ok());
        assert!(validate_actor_creations(&serde_json::json!({})).is_ok());
        for (path, value) in [
            ("/actor_creation_error", serde_json::json!("observer failed")),
            ("/actor_creation_inputs/1/actor", serde_json::json!("root")),
            ("/actor_creation_inputs/1/definition_id", serde_json::json!("def-1")),
            ("/actor_creation_inputs/1/parent", serde_json::json!("unknown")),
            ("/actor_creation_inputs/1/pointer_hex", serde_json::json!("0")),
            ("/actor_creation_inputs/1/ordinal", serde_json::json!(1)),
            ("/actor_creation_inputs/1/before_seq", serde_json::json!(2)),
            ("/actor_creation_inputs/1/provenance", serde_json::json!("observed-callback-order")),
        ] {
            let mut invalid = document.clone();
            if path == "/actor_creation_error" { invalid["actor_creation_error"] = value; }
            else { *invalid.pointer_mut(path).expect("creation field") = value; }
            assert!(validate_actor_creations(&invalid).is_err(), "accepted invalid {path}");
        }
        let mut missing = document.clone();
        missing["actor_creation_inputs"].as_array_mut().expect("creation inputs").pop();
        assert!(validate_actor_creations(&missing).is_err());
    }

    #[test]
    fn theme_inputs_require_initial_settings() {
        let document = serde_json::json!({
            "capabilities": {"native_initial_theme_modifiers": true},
            "input": {"gameplay_preferences": {"column_cue_min_time": 1.5,
                "players": [{"player": "PlayerNumber_P1", "theme_modifiers": {
                    "ColumnCues": true, "SpeedMod": 250, "ComboFont": "Wendy",
                    "TimingWindows": [true, true, true, true, true]}}]}}
        });
        assert!(validate_theme_inputs(&document).is_ok());
        assert!(validate_theme_inputs(&serde_json::json!({})).is_ok());
        for (path, value) in [
            ("/input/gameplay_preferences/column_cue_min_time", serde_json::json!(-1)),
            ("/input/gameplay_preferences/column_cue_min_time", Value::Null),
            ("/input/gameplay_preferences/players/0/player", serde_json::json!("unknown")),
            ("/input/gameplay_preferences/players/0/theme_modifiers", Value::Null),
            ("/input/gameplay_preferences/players/0/theme_modifiers/ColumnCues", serde_json::json!("true")),
            ("/input/gameplay_preferences/players/0/theme_modifiers/TimingWindows", serde_json::json!([{"type": "function"}])),
        ] {
            let mut invalid = document.clone();
            *invalid.pointer_mut(path).expect("initial theme input field") = value;
            assert!(validate_theme_inputs(&invalid).is_err(), "accepted invalid {path}");
        }
        let mut duplicate = document.clone();
        duplicate["input"]["gameplay_preferences"]["players"].as_array_mut().expect("players")
            .push(document["input"]["gameplay_preferences"]["players"][0].clone());
        assert!(validate_theme_inputs(&duplicate).is_err());
    }

    #[test]
    fn update_clocks_require_actual_frames() {
        assert!(validate_runtime_inputs(&serde_json::json!({"update_frames": []})).is_err());
        assert!(validate_runtime_inputs(&serde_json::json!({})).is_err());
    }

    #[test]
    fn fallback_theme_metric_uses_raw_theme_name() {
        let ini = metrics("Simply-Love-SM5");
        assert!(ini.contains("FallbackTheme=Simply-Love-SM5"));
        assert!(!ini.contains("FallbackTheme=\"Simply-Love-SM5\""));
    }

    #[test]
    fn native_fast_note_rendering_requires_observed_boolean_preferences() {
        let document = serde_json::json!({"capabilities":{"native_fast_note_rendering":true},
            "input":{"fast_note_rendering_requested":true,"fast_note_rendering":true,
                "fast_note_rendering_at_end":true}});
        validate_fast_note_rendering(&document).unwrap();
        let exported = notefield_render_document(document.clone(), serde_json::json!({}));
        assert_eq!(exported["capabilities"]["native_fast_note_rendering"], true);
        validate_fast_note_rendering(&exported).unwrap();
        for (field, value) in [("fast_note_rendering", Value::Null),
            ("fast_note_rendering_at_end", Value::Null),
            ("fast_note_rendering_requested", serde_json::json!("1")),
            ("fast_note_rendering_requested", serde_json::json!(false))]
        {
            let mut invalid = document.clone();
            invalid["input"][field] = value;
            assert!(validate_fast_note_rendering(&invalid).is_err(), "accepted {field}");
        }
        let mut native_default = document.clone();
        native_default["input"]["fast_note_rendering_requested"] = Value::Null;
        validate_fast_note_rendering(&native_default).unwrap();
        // These are observed initial/final values, not a claim that authored Lua
        // could not have changed the preference after gameplay initialization.
        native_default["input"]["fast_note_rendering_at_end"] = Value::Bool(false);
        validate_fast_note_rendering(&native_default).unwrap();
        validate_fast_note_rendering(&serde_json::json!({})).unwrap();
    }

    #[test]
    fn native_option_clock_deltas_survive_export_without_screen_delta_substitution() {
        let frames = serde_json::json!([
            {"sequence":1,"delta":0.125,"option_deltas":{"song":0.008_f32,
                "players":[0.009_f32,0.010_f32]}},
            {"sequence":2,"delta":0.25,"option_deltas":{"song":0.0,
                "players":[0.011_f32,0.012_f32]}}
        ]);
        let native = serde_json::json!({"capabilities":{"native_option_clock_deltas":true},
            "update_frames":frames});
        validate_option_clock_deltas(&native).unwrap();
        let exported = notefield_render_document(native.clone(), serde_json::json!({}));
        assert_eq!(exported["capabilities"]["native_option_clock_deltas"], true);
        assert_eq!(exported["update_frames"], native["update_frames"]);
        validate_option_clock_deltas(&exported).unwrap();

        for (pointer, value) in [
            ("/update_frames", serde_json::json!([])),
            ("/update_frames/0/option_deltas", Value::Null),
            ("/update_frames/1/option_deltas/song", Value::Null),
            ("/update_frames/1/option_deltas/song", serde_json::json!(-0.1_f32)),
            ("/update_frames/1/option_deltas/song", serde_json::json!(0.1_f64)),
            ("/update_frames/1/option_deltas/players", serde_json::json!([0.01_f32])),
            ("/update_frames/1/option_deltas/players", serde_json::json!([0.01_f32,0.01_f32,0.01_f32])),
            ("/update_frames/1/option_deltas/players/0", serde_json::json!("0.01")),
            ("/update_frames/1/option_deltas/players/1", serde_json::json!(-0.01_f32)),
        ] {
            let mut invalid = native.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(validate_option_clock_deltas(&invalid).is_err(), "accepted invalid {pointer}");
        }
        let legacy = notefield_render_document(serde_json::json!({}), serde_json::json!({}));
        assert!(legacy["capabilities"]["native_option_clock_deltas"].is_null());
        validate_option_clock_deltas(&legacy).unwrap();
    }

    #[test]
    fn native_gameplay_clock_inputs_require_explicit_frame_associated_raw_values() {
        let mut native = serde_json::json!({
            "capabilities":{"native_gameplay_clock_inputs":true},
            "input":{"native_source":{"screen_gameplay_sha256":"a".repeat(64)}},
            "update_frames":[
                {"sequence":1,"scene":1,"native_update_frame":0,"native_update_sequence":9,
                 "gameplay_clock_inputs":false},
                {"sequence":2,"scene":1,"native_update_frame":1,"native_update_sequence":10,
                 "delta":0.5,"gameplay_clock_inputs":{"scene":1,"native_update_frame":1,
                    "native_update_sequence":10,"phase":"after-gameplay-actors",
                    "delta_seconds":0.25_f32,"music_rate":1.5_f32,"steps_time_active":true}},
                {"sequence":3,"scene":1,"native_update_frame":2,"native_update_sequence":11,
                 "gameplay_clock_inputs":false}
            ]
        });
        assert!(validate_gameplay_clock_inputs(&native).is_err(), "raw Lua sentinel is not canonical JSON");
        normalize_gameplay_clock_inputs(&mut native);
        assert!(native["update_frames"][0]["gameplay_clock_inputs"].is_null());
        validate_gameplay_clock_inputs(&native).unwrap();
        let exported = notefield_render_document(native.clone(), serde_json::json!({}));
        assert_eq!(exported["capabilities"]["native_gameplay_clock_inputs"], true);
        assert_eq!(exported["update_frames"], native["update_frames"]);
        validate_gameplay_clock_inputs(&exported).unwrap();
        for (pointer, value) in [
            ("/capabilities/native_gameplay_clock_inputs", serde_json::json!(false)),
            ("/input/native_source/screen_gameplay_sha256", Value::Null),
            ("/update_frames", serde_json::json!([])),
            ("/update_frames/1/gameplay_clock_inputs", serde_json::json!(true)),
            ("/update_frames/1/gameplay_clock_inputs/scene", serde_json::json!(2)),
            ("/update_frames/1/gameplay_clock_inputs/native_update_frame", serde_json::json!(0)),
            ("/update_frames/1/gameplay_clock_inputs/native_update_sequence", serde_json::json!(9)),
            ("/update_frames/1/gameplay_clock_inputs/phase", serde_json::json!("before-gameplay-actors")),
            ("/update_frames/1/gameplay_clock_inputs/delta_seconds", serde_json::json!(-0.25_f32)),
            ("/update_frames/1/gameplay_clock_inputs/delta_seconds", serde_json::json!(0.1_f64)),
            ("/update_frames/1/gameplay_clock_inputs/music_rate", serde_json::json!(0.1_f64)),
            ("/update_frames/1/gameplay_clock_inputs/steps_time_active", serde_json::json!(1)),
        ] {
            let mut invalid = native.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(validate_gameplay_clock_inputs(&invalid).is_err(), "accepted invalid {pointer}");
        }
        let mut missing = native.clone();
        missing["update_frames"][0].as_object_mut().unwrap().remove("gameplay_clock_inputs");
        normalize_gameplay_clock_inputs(&mut missing);
        assert!(validate_gameplay_clock_inputs(&missing).is_err(), "missing evidence became a no-call input");
        let mut repeated = native.clone();
        repeated["update_frames"][2] = repeated["update_frames"][1].clone();
        repeated["update_frames"][2]["sequence"] = serde_json::json!(3);
        assert!(validate_gameplay_clock_inputs(&repeated).is_err(), "reused a native timer occurrence");
        let legacy = notefield_render_document(serde_json::json!({}), serde_json::json!({}));
        assert!(legacy["capabilities"]["native_gameplay_clock_inputs"].is_null());
        validate_gameplay_clock_inputs(&legacy).unwrap();
    }

    #[test]
    fn native_arrow_cache_clocks_preserve_independent_double_inputs() {
        let native = serde_json::json!({
            "capabilities":{"native_arrow_update_clocks":true},
            "update_frames":[
                {"sequence":1,"native_update_frame":2,"song_beat":-1.0},
                {"sequence":2,"native_update_frame":3,"song_beat":0.0,
                 "wall_seconds":17.0,"delta":0.5,
                 "arrow_clock_inputs":{"update_seconds":17.125000001,
                    "players":[{"tipsy_seconds":17.125000002,"tan_tipsy_seconds":17.125000003},
                        {"tipsy_seconds":17.125000004,"tan_tipsy_seconds":17.125000005}]}}
            ]
        });
        validate_arrow_update_clocks(&native).unwrap();
        let exported = notefield_render_document(native.clone(), serde_json::json!({}));
        assert_eq!(exported["capabilities"]["native_arrow_update_clocks"], true);
        assert_eq!(exported["update_frames"], native["update_frames"]);
        validate_arrow_update_clocks(&exported).unwrap();
        for (pointer, value) in [
            ("/update_frames", serde_json::json!([])),
            ("/update_frames/1/arrow_clock_inputs", Value::Null),
            ("/update_frames/1/arrow_clock_inputs/update_seconds", serde_json::json!(-1.0)),
            ("/update_frames/1/arrow_clock_inputs/players", serde_json::json!([{}])),
            ("/update_frames/1/arrow_clock_inputs/players/0", serde_json::json!(7)),
            ("/update_frames/1/arrow_clock_inputs/players/1/tipsy_seconds", serde_json::json!("17")),
            ("/update_frames/1/arrow_clock_inputs/players/1/tan_tipsy_seconds", serde_json::json!(-1.0)),
        ] {
            let mut invalid = native.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(validate_arrow_update_clocks(&invalid).is_err(), "accepted invalid {pointer}");
        }
        let mut inactive = native.clone();
        inactive["update_frames"][1]["arrow_clock_inputs"]["players"] = serde_json::json!([{}, []]);
        validate_arrow_update_clocks(&inactive).unwrap();
        let legacy = notefield_render_document(serde_json::json!({}), serde_json::json!({}));
        assert!(legacy["capabilities"]["native_arrow_update_clocks"].is_null());
        validate_arrow_update_clocks(&legacy).unwrap();
    }

    #[test]
    fn native_player_current_positions_keep_player_and_draw_frame_identity() {
        let native = serde_json::json!({
            "capabilities":{"native_player_actor_positions":true},
            "input":{"players":[{"player":"PlayerNumber_P1"},{"player":"PlayerNumber_P2"}]},
            "update_frames":[{"sequence":1,"scene":1,"native_update_frame":7}],
            "model_geometry_frames":[{"scene":1,"update_sequence":1,"samples":0}],
            "player_actor_position_samples":[{"scene":1,"update_sequence":1,"native_update_frame":7,
                "players":[{"player":0,"name":"PlayerP1","x":107.0,"y":240.0,"z":0.0},
                    {"player":1,"name":"PlayerP2","x":747.0,"y":240.0,"z":0.0}]}]
        });
        validate_player_actor_positions(&native).unwrap();
        let exported = notefield_render_document(native.clone(), serde_json::json!({}));
        assert_eq!(exported["capabilities"]["native_player_actor_positions"], true);
        assert_eq!(exported["player_actor_position_samples"], native["player_actor_position_samples"]);
        validate_player_actor_positions(&exported).unwrap();
        for (pointer, value) in [
            ("/player_actor_position_samples", serde_json::json!([])),
            ("/player_actor_position_samples/0/update_sequence", serde_json::json!(2)),
            ("/player_actor_position_samples/0/native_update_frame", serde_json::json!(6)),
            ("/player_actor_position_samples/0/players/1/player", serde_json::json!(0)),
            ("/player_actor_position_samples/0/players/1/name", serde_json::json!("PlayerP1")),
            ("/player_actor_position_samples/0/players/1/x", serde_json::json!(0.1_f64)),
            ("/player_actor_position_samples/0/players/1/y", Value::Null),
        ] {
            let mut invalid = native.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(validate_player_actor_positions(&invalid).is_err(), "accepted invalid {pointer}");
        }
        let legacy = notefield_render_document(serde_json::json!({}), serde_json::json!({}));
        assert!(legacy["capabilities"]["native_player_actor_positions"].is_null());
        validate_player_actor_positions(&legacy).unwrap();
    }

    #[test]
    fn native_player_current_options_keep_canonical_values_and_nullable_getters() {
        let options = serde_json::json!({
            "perspective_tilt":-2.0,"skew":0.0,"mini":-0.125,"brake":1.0,"drunk":0.0,"tipsy":0.0,
            "approach_speeds":{"perspective_tilt":20.0,"skew":20.0,"mini":4.0,
                "brake":1.0,"drunk":1.0,"tipsy":1.0},
            "perspective_getters":{
                "Hallway":{"present":true,"value":2.0,"approach_speed":20.0},
                "Distant":{"present":false},"Incoming":{"present":false},"Space":{"present":false}}
        });
        let mut other = options.clone();
        other["perspective_tilt"] = serde_json::json!(-0.5);
        other["skew"] = serde_json::json!(-0.5);
        other["perspective_getters"]["Hallway"] = serde_json::json!({"present":false});
        other["perspective_getters"]["Space"] = serde_json::json!({"present":true,"value":-0.5,"approach_speed":20.0});
        let native = serde_json::json!({
            "capabilities":{"native_player_actor_positions":true,"native_player_current_options":true},
            "input":{"players":[{"player":"PlayerNumber_P1"},{"player":"PlayerNumber_P2"}]},
            "update_frames":[{"sequence":1,"scene":1,"native_update_frame":7}],
            "model_geometry_frames":[{"scene":1,"update_sequence":1,"samples":0}],
            "player_actor_position_samples":[{"scene":1,"update_sequence":1,"native_update_frame":7,
                "players":[{"player":0,"name":"PlayerP1","x":107.0,"y":240.0,"z":0.0,"current_options":options},
                    {"player":1,"name":"PlayerP2","x":747.0,"y":240.0,"z":0.0,"current_options":other}]}]
        });
        validate_player_current_options(&native).unwrap();
        let exported = notefield_render_document(native.clone(), serde_json::json!({}));
        assert_eq!(exported["capabilities"]["native_player_current_options"], true);
        assert_eq!(exported["player_actor_position_samples"], native["player_actor_position_samples"]);
        validate_player_current_options(&exported).unwrap();
        for (pointer, value) in [
            ("/capabilities/native_player_actor_positions", serde_json::json!(false)),
            ("/player_actor_position_samples/0/native_update_frame", serde_json::json!(6)),
            ("/player_actor_position_samples/0/players/0/current_options/perspective_tilt", Value::Null),
            ("/player_actor_position_samples/0/players/0/current_options/mini", serde_json::json!(0.1_f64)),
            ("/player_actor_position_samples/0/players/0/current_options/approach_speeds/skew", serde_json::json!(-1)),
            ("/player_actor_position_samples/0/players/0/current_options/perspective_getters/Hallway/present", serde_json::json!(false)),
            ("/player_actor_position_samples/0/players/0/current_options/perspective_getters/Hallway/approach_speed", Value::Null),
            ("/player_actor_position_samples/0/players/0/current_options/perspective_getters/Distant/present", Value::Null),
        ] {
            let mut invalid = native.clone();
            *invalid.pointer_mut(pointer).unwrap() = value;
            assert!(validate_player_current_options(&invalid).is_err(), "accepted invalid {pointer}");
        }
        let legacy = notefield_render_document(serde_json::json!({}), serde_json::json!({}));
        assert!(legacy["capabilities"]["native_player_current_options"].is_null());
        validate_player_current_options(&legacy).unwrap();
    }

    #[test]
    fn song_position_clocks_keep_raw_and_visible_values_and_profile_aliases() {
        let clock = serde_json::json!({"seconds":2.0,"song_beat":4.0,
            "visible_seconds":1.875,"visible_beat":3.75,"beat":3.75});
        let native = serde_json::json!({"capabilities":{"native_song_position_clocks":true,
            "native_pre_theme_preferences":true},
            "input":{"notefield_samples":true,
                "preferences":{"global_offset_seconds":-0.008,"visual_delay_seconds":0.125,
                    "music_rate":1,"default_sync_offset":"SyncOffset_ITG"},
                "catalog_preferences":{"initialization":"native-catalog-note-time-calculation",
                    "global_offset_seconds":-0.008,"music_rate":1},
                "players":[{"player":"PlayerNumber_P1","preferred_visual_delay_seconds":0}]},
            "update_frames":[clock.clone()],"end_position":clock});
        validate_song_position_clocks(&native).unwrap();
        let exported = notefield_render_document(native.clone(), serde_json::json!({}));
        assert_eq!(exported["capabilities"]["native_song_position_clocks"], true);
        assert_eq!(exported["capabilities"]["native_pre_theme_preferences"], true);
        validate_song_position_clocks(&exported).unwrap();
        for pointer in ["/update_frames/0/song_beat", "/update_frames/0/visible_seconds",
            "/update_frames/0/visible_beat", "/end_position/song_beat",
            "/input/preferences/global_offset_seconds", "/input/preferences/visual_delay_seconds",
            "/input/preferences/default_sync_offset", "/input/preferences/music_rate",
            "/input/players/0/preferred_visual_delay_seconds", "/input/catalog_preferences/music_rate"]
        {
            let mut invalid = native.clone();
            *invalid.pointer_mut(pointer).unwrap() = Value::Null;
            assert!(validate_song_position_clocks(&invalid).is_err(), "accepted missing {pointer}");
        }
        let mut wrong_alias = native.clone();
        wrong_alias["update_frames"][0]["beat"] = serde_json::json!(4.0);
        assert!(validate_song_position_clocks(&wrong_alias).is_err());
        let mut semantic = native.clone();
        semantic["input"]["notefield_samples"] = Value::Bool(false);
        semantic["input"]["gameplay_preferences"] = serde_json::json!({"players":native["input"]["players"]});
        semantic["update_frames"][0]["beat"] = serde_json::json!(4.0);
        semantic["end_position"]["beat"] = serde_json::json!(4.0);
        validate_song_position_clocks(&semantic).unwrap();
        semantic["update_frames"][0]["beat"] = serde_json::json!(3.75);
        assert!(validate_song_position_clocks(&semantic).is_err());
        validate_song_position_clocks(&serde_json::json!({})).unwrap();
    }

    #[test]
    fn headless_drawables_require_real_graphics_and_offscreen_adapter() {
        let preferences = preferences(Path::new("fixture"), true, true).unwrap();
        assert!(preferences.contains("VideoRenderers=opengl\n"));
        assert!(preferences.contains("ShowLoadingWindow=0\n"));
        assert!(!preferences.contains("VideoRenderers=null"));
        assert!(require_headless_graphics_adapter(b"old executable").is_err());
        assert!(require_headless_graphics_adapter(b"ITGMANIA_HARNESS_HEADLESS_GRAPHICS_AVAILABLE").is_ok());
    }

    #[test]
    fn stages_an_isolated_portable_probe_install() {
        let fixture = create_run_dir().unwrap();
        let install = fixture.join("install");
        let program = install.join("Program");
        let data = install.join("Data");
        let fallback = install.join("Themes/Simply-Love-SM5");
        let song = fixture.join("source-song");
        for path in [&program, &data, &fallback, &song] {
            create_dir(path).unwrap();
        }
        write(&program.join("ITGmania.exe"), b"binary").unwrap();
        write(&program.join("runtime.dll"), b"runtime").unwrap();
        write(&data.join("Defaults.ini"), b"[Options]\n").unwrap();
        create_dir(&fallback.join("Scripts")).unwrap();
        write(&fallback.join("Scripts/SL_Init.lua"), b"SL = {}\n").unwrap();
        create_dir(&fallback.join("Fonts/_Combo Fonts/Wendy")).unwrap();
        write(&fallback.join("Fonts/_Combo Fonts/Wendy/Wendy.ini"), b"[main]\n").unwrap();
        write(&fallback.join("Fonts/_Combo Fonts/Wendy/Wendy 8x2.png"), b"combo atlas").unwrap();
        create_dir(&fallback.join("Graphics/_Judgments")).unwrap();
        write(&fallback.join("Graphics/_Judgments/Love 2x6.png"), b"judgment atlas").unwrap();
        write(&song.join("song.ssc"), b"#TITLE:Probe;\n").unwrap();
        write(&song.join("song.sm"), b"#TITLE:Requested SM;\n").unwrap();
        write(&song.join("other.SSC"), b"#TITLE:Competing SSC;\n").unwrap();
        create_dir(&song.join("lua")).unwrap();
        write(&song.join("lua/default.lua"), b"return Def.ActorFrame{}\n").unwrap();
        write(&song.join("lua/nested.ssc"), b"nested resource\n").unwrap();

        let options = Options {
            simfile: song.join("song.ssc"),
            itgmania_bin: program.join("ITGmania.exe"),
            until_beat: 4.0,
            game: "dance".into(),
            chart_index: None,
            style: "single".into(),
            difficulty: None,
            fallback_theme: "Simply-Love-SM5".into(),
            timeout: Duration::from_secs(30),
            keep_run_dir: false,
            headless: true,
            until_gameplay_end: false,
            autoplay: true,
            max_events: DEFAULT_MAX_EVENTS,
            random_seed: 1,
            inputs: Vec::new(),
            drawable_samples: false,
            capture_fps: None,
            fixed_update_fps: Some(60),
            stop_lua_gc: false,
            notefield_samples: false,
            scene_draws: false,
            render_target_passes: false,
            drawable_from_beat: 0.0,
            player_options: None,
            fast_note_rendering: Some(true),
        };
        let run_dir = fixture.join("run");
        create_dir(&run_dir).unwrap();
        prepare_install(
            &options,
            &song.join("song.ssc"),
            &program.join("ITGmania.exe"),
            &install,
            &run_dir,
        )
        .unwrap();

        assert!(run_dir.join("Portable.ini").is_file());
        assert!(run_dir.join("Program/runtime.dll").is_file());
        assert!(run_dir.join("Songs/Harness/Probe/song.ssc").is_file());
        assert!(!run_dir.join("Songs/Harness/Probe/song.sm").exists());
        assert!(!run_dir.join("Songs/Harness/Probe/other.SSC").exists());
        assert!(run_dir.join("Songs/Harness/Probe/lua/default.lua").is_file());
        assert!(run_dir.join("Songs/Harness/Probe/lua/nested.ssc").is_file());
        assert!(run_dir
            .join("Themes/itgmania-harness/Scripts/SL_Init.lua")
            .is_file());
        let theme = run_dir.join("Themes/itgmania-harness");
        assert_eq!(fs::read(theme.join("Fonts/_Combo Fonts/Wendy/Wendy.ini")).unwrap(), b"[main]\n");
        assert_eq!(fs::read(theme.join("Fonts/_Combo Fonts/Wendy/Wendy 8x2.png")).unwrap(), b"combo atlas");
        assert_eq!(fs::read(theme.join("Graphics/_Judgments/Love 2x6.png")).unwrap(), b"judgment atlas");
        assert!(run_dir
            .join("Themes/itgmania-harness/BGAnimations/ScreenGameplay overlay.lua")
            .is_file());
        let preferences = fs::read_to_string(run_dir.join("Save/Preferences.ini")).unwrap();
        assert!(preferences.contains("AdditionalFoldersReadOnly="));
        assert!(preferences.contains("TestInitialScreen=ScreenSongLuaHarness"));
        assert!(preferences.contains("VideoRenderers=null"));
        let delta = preferences.lines().find_map(|line| line.strip_prefix("ConstantUpdateDeltaSeconds="))
            .unwrap().parse::<f64>().unwrap();
        assert_eq!(delta as f32, 1.0_f32 / 60.0);
        let sm_run = fixture.join("sm-run");
        create_dir(&sm_run).unwrap();
        let sm = song.join("song.sm");
        let sm_options = Options { simfile: sm.clone(), ..options };
        prepare_install(&sm_options, &sm, &program.join("ITGmania.exe"), &install, &sm_run).unwrap();
        assert!(sm_run.join("Songs/Harness/Probe/song.sm").is_file());
        assert!(!sm_run.join("Songs/Harness/Probe/song.ssc").exists());
        assert!(!sm_run.join("Songs/Harness/Probe/other.SSC").exists());
        assert!(sm_run.join("Songs/Harness/Probe/lua/nested.ssc").is_file());
        assert_eq!(fs::read(song.join("song.ssc")).unwrap(), b"#TITLE:Probe;\n");
        assert_eq!(fs::read(song.join("song.sm")).unwrap(), b"#TITLE:Requested SM;\n");
        fs::remove_dir_all(fixture).unwrap();
    }
}
    #[test]
    fn render_target_passes_order_nested_empty_and_repeated_draws() {
        let pass = |sequence, kind, cursor, target, parent, active, name, preserve| {
            serde_json::json!({
                "sequence": sequence, "kind": kind, "after_native_draw_sequence": cursor,
                "target": target, "parent_target": parent, "active_target": active,
                "name": name, "preserve": preserve, "scene": 1, "native_update_sequence": 2,
                "draw_phase": "during_update"
            })
        };
        let sample = |sequence, target| serde_json::json!({
            "state":{"native_draw_sequence":sequence,"render_target":target}
        });
        let mut trace = serde_json::json!({
            "capabilities":{"native_render_target_pass_protocol":1,"native_drawable_vertices":true},
            "input":{"native_source":{"render_target_sha256":"a".repeat(64)}},
            "dropped_render_target_passes":0,
            "render_target_passes":[
                pass(1,"begin",10,1,0,1,"screen",Some(false)),
                pass(2,"begin",11,2,1,2,"blur",Some(true)),
                pass(3,"end",11,2,1,1,"blur",None),
                pass(4,"end",13,1,0,0,"screen",None),
                pass(5,"begin",13,2,0,2,"blur",Some(false)),
                pass(6,"end",14,2,0,0,"blur",None)
            ],
            "drawable_samples":[sample(10,0),sample(11,1),sample(13,1),sample(14,2),sample(16,0)]
        });
        validate_render_target_passes(&trace).unwrap();
        for (path, bad) in [
            ("/render_target_passes/1/after_native_draw_sequence", Value::from(9)),
            ("/render_target_passes/2/name", Value::from("screen")),
            ("/render_target_passes/2/active_target", Value::from(0)),
            ("/render_target_passes/4/preserve", Value::Null),
            ("/drawable_samples/2/state/render_target", Value::from(0)),
            ("/drawable_samples/2/state/native_draw_sequence", Value::from(11)),
            ("/input/native_source/render_target_sha256", Value::Null),
            ("/dropped_render_target_passes", Value::from(1)),
        ] {
            let mut invalid = trace.clone();
            *invalid.pointer_mut(path).unwrap() = bad;
            assert!(validate_render_target_passes(&invalid).is_err(), "accepted {path}");
        }
        trace["render_target_passes"].as_array_mut().unwrap().pop();
        assert!(validate_render_target_passes(&trace).is_err());
    }
