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

    let started = Instant::now();
    loop {
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
            let _ = child.kill();
            let _ = child.wait();
            if let Some(message) = document.get("error").and_then(Value::as_str) {
                return Err(Error::new(format!(
                    "ITGmania probe script reported: {message}"
                )));
            }
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
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::new(format!(
                "ITGmania did not write a semantic trace within {:.1} seconds{}",
                options.timeout.as_secs_f64(),
                log_hint(run_dir)
            )));
        }
        thread::sleep(Duration::from_millis(25));
    }
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
        preferences(install_root).as_bytes(),
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

fn preferences(install_root: &Path) -> String {
    format!(
        "[Options]\n\
         AdditionalFoldersReadOnly={}\n\
         AllowMultipleInstances=1\n\
         FastLoad=0\n\
         LogToDisk=1\n\
         ShowLogOutput=0\n\
         SoundDrivers=Null\n\
         TestInitialScreen=ScreenSongLuaHarness\n\
         Theme={THEME_NAME}\n\
         Windowed=1\n\
         DisplayWidth=854\n\
         DisplayHeight=480\n",
        slash_path(install_root)
    )
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
         \tresult_path = \"/Save/itgmania-harness/song-lua-trace.json\",\n\
         \tuntil_beat = {:.9},\n\
         \tmax_events = 250000,\n\
        }}\n",
        lua_quote(env!("CARGO_PKG_VERSION")),
        lua_quote(&options.style),
        difficulty,
        lua_quote(&options.fallback_theme),
        lua_quote(&base_overlay),
        options.until_beat
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
        };
        let lua = config_lua(&options);
        assert!(lua.contains("schema_version = 1"));
        assert!(lua.contains("until_beat = 4.500000000"));
        assert!(lua.contains("difficulty = \"Challenge\""));
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
        fs::remove_dir_all(fixture).unwrap();
    }
}
