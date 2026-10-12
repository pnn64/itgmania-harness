use serde_json::Value;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const THEME_NAME: &str = "itgmania-harness";
const RESULT_PATH: &str = "Save/itgmania-harness/song-lua-trace.json";
const INIT_LUA: &str = include_str!("../probe_theme/ScreenSongLuaHarness overlay.lua");
const GAMEPLAY_LUA: &str = include_str!("../probe_theme/ScreenGameplay overlay.lua");
const SYSTEM_LUA: &str = include_str!("../probe_theme/ScreenSystemLayer overlay.lua");
const TRACE_LUA: &str = include_str!("../probe_theme/SongLuaTrace.lua");

#[derive(Debug)]
pub struct Options {
    pub simfile: PathBuf,
    pub itgmania_bin: PathBuf,
    pub until_beat: f64,
    pub style: String,
    pub difficulty: Option<String>,
    pub fallback_theme: String,
    pub timeout: Duration,
    pub keep_run_dir: bool,
    pub headless: bool,
    pub until_gameplay_end: bool,
    pub autoplay: bool,
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
    let mut child = Command::new(&staged_binary)
        .args(["--game=dance", &format!("--theme={THEME_NAME}")])
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
            let bytes = fs::read(&result_path).map_err(|source| {
                Error::new(format!(
                    "could not read runtime result {}: {source}",
                    result_path.display()
                ))
            })?;
            let mut document: Value = serde_json::from_slice(&bytes).map_err(|source| {
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
            if options.headless
                && document.pointer("/display/video_renderers").and_then(Value::as_str) != Some("null")
            {
                return Err(Error::new("ITGmania did not retain the requested null renderer"));
            }
            if options.until_gameplay_end {
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
            validate_runtime_inputs(&document)?;
            document["source_simfile"] = Value::String(slash_path(&options.simfile));
            crate::song_lua_semantics::enrich(&mut document).map_err(|source| {
                Error::new(format!("could not derive song Lua semantics: {source}"))
            })?;
            return Ok(document);
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
    )?;
    copy_dir(&data, &run_dir.join("Data"), true)?;
    copy_dir(
        simfile
            .parent()
            .ok_or_else(|| Error::new("simfile has no song directory"))?,
        &run_dir.join("Songs/Harness/Probe"),
        true,
    )?;
    copy_dir(
        &fallback.join("Scripts"),
        &run_dir.join("Themes/itgmania-harness/Scripts"),
        true,
    )?;

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
    write(
        &run_dir.join("Save/Preferences.ini"),
        preferences(install_root, options.headless)?.as_bytes(),
    )?;
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
    write(
        &run_dir.join("Themes/itgmania-harness/Scripts/03 SongLuaTrace.lua"),
        TRACE_LUA.as_bytes(),
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

fn preferences(install_root: &Path, headless: bool) -> Result<String, Error> {
    // CheckVideoDefaultSettings replaces VideoRenderers when this differs.
    let renderer = if headless {
        format!("VideoRenderers=null\nLastSeenVideoDriver={}\nShowLoadingWindow=0\n", video_driver()?)
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
    validate_gameplay_preferences(document)?;
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
            if source.is_none_or(str::is_empty) || line.is_none_or(|line| line < -1) {
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
            if event.get("kind").and_then(Value::as_str) == Some("query")
                && event.pointer("/detail/results").and_then(Value::as_array).is_none()
            {
                return Err(Error::new("native query capture has no result tuple"));
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
    Ok(())
}

fn config_lua(options: &Options) -> String {
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
         \tstyle = {},\n\
         \tdifficulty = {},\n\
         \tfallback_theme = {},\n\
         \tbase_gameplay_overlay = {},\n\
         \tbase_system_overlay = {},\n\
         \tresult_path = \"/Save/itgmania-harness/song-lua-trace.json\",\n\
         \tuntil_beat = {:.9},\n\
         \tuntil_gameplay_end = {},\n\
         \tautoplay = {},\n\
         \tmax_events = 250000,\n\
        }}\n",
        lua_quote(env!("CARGO_PKG_VERSION")),
        lua_quote(&options.style),
        difficulty,
        lua_quote(&options.fallback_theme),
        lua_quote(&base_overlay),
        lua_quote(&format!("/Themes/{}/BGAnimations/ScreenSystemLayer overlay", options.fallback_theme)),
        options.until_beat,
        options.until_gameplay_end,
        u8::from(options.autoplay)
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

fn copy_dir(source: &Path, destination: &Path, recursive: bool) -> Result<(), Error> {
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
                copy_dir(&source_path, &destination_path, true)?;
            }
        } else if metadata.is_file() {
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
    use super::*;

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
        let options = Options {
            simfile: PathBuf::from("song.ssc"),
            itgmania_bin: PathBuf::from("ITGmania.exe"),
            until_beat: 4.5,
            style: "single".into(),
            difficulty: Some("Challenge".into()),
            fallback_theme: "Simply-Love-SM5".into(),
            timeout: Duration::from_secs(30),
            keep_run_dir: false,
            headless: false,
            until_gameplay_end: false,
            autoplay: true,
        };
        let lua = config_lua(&options);
        assert!(lua.contains("schema_version = 1"));
        assert!(lua.contains("until_beat = 4.500000000"));
        assert!(lua.contains("difficulty = \"Challenge\""));
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
        for (pointer, value) in [
            ("/events/1/caller", Value::Null),
            ("/events/1/caller/source", serde_json::json!("")),
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
        write(&song.join("song.ssc"), b"#TITLE:Probe;\n").unwrap();

        let options = Options {
            simfile: song.join("song.ssc"),
            itgmania_bin: program.join("ITGmania.exe"),
            until_beat: 4.0,
            style: "single".into(),
            difficulty: None,
            fallback_theme: "Simply-Love-SM5".into(),
            timeout: Duration::from_secs(30),
            keep_run_dir: false,
            headless: true,
            until_gameplay_end: false,
            autoplay: true,
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
        assert!(run_dir
            .join("Themes/itgmania-harness/Scripts/SL_Init.lua")
            .is_file());
        assert!(run_dir
            .join("Themes/itgmania-harness/BGAnimations/ScreenGameplay overlay.lua")
            .is_file());
        let preferences = fs::read_to_string(run_dir.join("Save/Preferences.ini")).unwrap();
        assert!(preferences.contains("AdditionalFoldersReadOnly="));
        assert!(preferences.contains("TestInitialScreen=ScreenSongLuaHarness"));
        assert!(preferences.contains("VideoRenderers=null"));
        fs::remove_dir_all(fixture).unwrap();
    }
}
