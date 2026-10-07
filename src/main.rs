use std::env;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

mod actor_conformance;
mod baseline;
mod chart_report;
mod font_baseline;
mod font_oracle;
mod json_diff;
mod noteskin_baseline;
mod noteskin_oracle;
mod oracle;
mod selector;
mod song_lua_archive;
mod song_lua_baseline;
mod song_lua_headless;
mod song_lua_oracle;
mod song_lua_runtime;
mod song_lua_semantic_baseline;
mod song_lua_semantics;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_FONT_TEXT: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

const CORE_FILES: &[(&str, &str)] = &[
    ("Song", "src/Song.h"),
    ("Steps", "src/Steps.h"),
    ("TimingData", "src/TimingData.h"),
    ("SM loader", "src/NotesLoaderSM.h"),
    ("SSC loader", "src/NotesLoaderSSC.h"),
    ("Font", "src/Font.h"),
    ("Actor", "src/Actor.h"),
    ("ActorFrame", "src/ActorFrame.h"),
    ("Tween", "src/Tween.h"),
];

const THEME_SCRIPT_PAIRS: &[(&str, &str)] = &[
    (
        "Themes/Simply Love/Scripts/SL-ChartParser.lua",
        "Themes/Simply Love/Scripts/SL-ChartParserHelpers.lua",
    ),
    (
        "Themes/Simply-Love-SM5/Scripts/SL-ChartParser.lua",
        "Themes/Simply-Love-SM5/Scripts/SL-ChartParserHelpers.lua",
    ),
    (
        "Themes/Arrow Cloud/Scripts/SL-ChartParser.lua",
        "Themes/Arrow Cloud/Scripts/SL-ChartParserHelpers.lua",
    ),
];

fn main() -> ExitCode {
    match run(env::args_os().skip(1)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), CliError> {
    let mut args = args.peekable();
    let Some(command) = args.next() else {
        print_help();
        return Ok(());
    };

    match command.to_str() {
        Some("doctor") => doctor(parse_doctor_args(args)?),
        Some("actor-conformance") => emit_actor_conformance(parse_actor_conformance_args(args)?),
        Some("charts") => emit_charts(parse_chart_args(args, false)?),
        Some("chart") => emit_charts(parse_chart_args(args, true)?),
        Some("font") => emit_font(parse_font_args(args)?),
        Some("font-baseline") => run_font_baseline(parse_font_baseline_args(args)?),
        Some("noteskin") => emit_noteskin(parse_noteskin_args(args)?),
        Some("noteskin-baseline") => run_noteskin_baseline(parse_noteskin_baseline_args(args)?),
        Some("song-lua") => emit_song_lua(parse_song_lua_args(args)?),
        Some("song-lua-baseline") => run_song_lua_baseline(parse_song_lua_baseline_args(args)?),
        Some("song-lua-archive") => run_song_lua_archive(parse_song_lua_archive_args(args)?),
        Some("song-lua-semantic-baseline") => {
            run_song_lua_semantic_baseline(parse_song_lua_semantic_baseline_args(args)?)
        }
        Some("song-lua-trace") => emit_song_lua_trace(parse_song_lua_trace_args(args)?),
        Some("song-lua-analyze") => emit_song_lua_analysis(parse_song_lua_analysis_args(args)?),
        Some("baseline") => run_baseline(parse_manifest_dir_args(args, "baseline", "--out")?),
        Some("verify") => run_verify(parse_manifest_dir_args(args, "verify", "--against")?),
        Some("diff") => run_diff(parse_diff_args(args)?),
        Some("--help" | "-h" | "help") => {
            print_help();
            Ok(())
        }
        Some("--version" | "-V" | "version") => {
            println!("{VERSION}");
            Ok(())
        }
        Some(command) => Err(CliError::Usage(format!("unknown command `{command}`"))),
        None => Err(CliError::Usage("command is not valid UTF-8".into())),
    }
}

struct SongLuaTraceArgs {
    options: song_lua_runtime::Options,
    out: Option<PathBuf>,
}

struct ActorConformanceArgs {
    fixture: PathBuf,
    out: Option<PathBuf>,
}

fn parse_actor_conformance_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<ActorConformanceArgs, CliError> {
    let fixture = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("actor-conformance requires a fixture JSON file".into()))?;
    let mut out = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out") => set_once(
                &mut out,
                PathBuf::from(next_utf8(&mut args, "--out")?),
                "--out",
            )?,
            Some(option) => {
                return Err(CliError::Usage(format!(
                    "unknown actor-conformance option `{option}`"
                )));
            }
            None => {
                return Err(CliError::Usage(
                    "actor-conformance option is not valid UTF-8".into(),
                ));
            }
        }
    }
    Ok(ActorConformanceArgs { fixture, out })
}

fn emit_actor_conformance(args: ActorConformanceArgs) -> Result<(), CliError> {
    let document =
        actor_conformance::evaluate(&args.fixture).map_err(CliError::ActorConformance)?;
    emit_json_document(&document, args.out)
}

fn parse_song_lua_trace_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<SongLuaTraceArgs, CliError> {
    let simfile = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("song-lua-trace command requires a simfile".into()))?;
    let mut itgmania_bin = None;
    let mut until_beat = None;
    let mut style = None;
    let mut difficulty = None;
    let mut fallback_theme = None;
    let mut timeout = None;
    let mut out = None;
    let mut keep_run_dir = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--itgmania-bin") => set_once(
                &mut itgmania_bin,
                PathBuf::from(next_utf8(&mut args, "--itgmania-bin")?),
                "--itgmania-bin",
            )?,
            Some("--until-beat") => {
                let value = next_utf8(&mut args, "--until-beat")?;
                let beat = value.parse::<f64>().map_err(|_| {
                    CliError::Usage(format!("trace end beat `{value}` is not a number"))
                })?;
                if !beat.is_finite() || beat < 0.0 {
                    return Err(CliError::Usage(
                        "trace end beat must be a finite, non-negative number".into(),
                    ));
                }
                set_once(&mut until_beat, beat, "--until-beat")?;
            }
            Some("--style") => set_once(&mut style, next_utf8(&mut args, "--style")?, "--style")?,
            Some("--difficulty") => set_once(
                &mut difficulty,
                next_utf8(&mut args, "--difficulty")?,
                "--difficulty",
            )?,
            Some("--fallback-theme") => set_once(
                &mut fallback_theme,
                next_utf8(&mut args, "--fallback-theme")?,
                "--fallback-theme",
            )?,
            Some("--timeout") => {
                let value = next_utf8(&mut args, "--timeout")?;
                let seconds = value.parse::<f64>().map_err(|_| {
                    CliError::Usage(format!("runtime timeout `{value}` is not a number"))
                })?;
                if !seconds.is_finite() || seconds <= 0.0 {
                    return Err(CliError::Usage(
                        "runtime timeout must be a finite number greater than zero".into(),
                    ));
                }
                set_once(
                    &mut timeout,
                    std::time::Duration::from_secs_f64(seconds),
                    "--timeout",
                )?;
            }
            Some("--out") => set_once(
                &mut out,
                PathBuf::from(next_utf8(&mut args, "--out")?),
                "--out",
            )?,
            Some("--keep-run-dir") if !keep_run_dir => keep_run_dir = true,
            Some("--keep-run-dir") => {
                return Err(CliError::Usage(
                    "--keep-run-dir may only be provided once".into(),
                ));
            }
            Some(option) => {
                return Err(CliError::Usage(format!(
                    "unknown song-lua-trace option `{option}`"
                )));
            }
            None => {
                return Err(CliError::Usage(
                    "song-lua-trace option is not valid UTF-8".into(),
                ));
            }
        }
    }
    Ok(SongLuaTraceArgs {
        options: song_lua_runtime::Options {
            simfile,
            itgmania_bin: itgmania_bin
                .ok_or_else(|| CliError::Usage("song-lua-trace requires --itgmania-bin".into()))?,
            until_beat: until_beat.unwrap_or(16.0),
            style: style.unwrap_or_else(|| "single".into()),
            difficulty,
            fallback_theme: fallback_theme.unwrap_or_else(|| "Simply-Love-SM5".into()),
            timeout: timeout.unwrap_or_else(|| std::time::Duration::from_secs(120)),
            keep_run_dir,
        },
        out,
    })
}

fn emit_song_lua_trace(args: SongLuaTraceArgs) -> Result<(), CliError> {
    let document = song_lua_runtime::capture(&args.options).map_err(CliError::SongLuaRuntime)?;
    emit_json_document(&document, args.out)
}

struct SongLuaAnalysisArgs {
    trace: PathBuf,
    out: Option<PathBuf>,
}

fn parse_song_lua_analysis_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<SongLuaAnalysisArgs, CliError> {
    let trace = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("song-lua-analyze requires a semantic trace".into()))?;
    let mut out = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out") => set_once(
                &mut out,
                PathBuf::from(next_utf8(&mut args, "--out")?),
                "--out",
            )?,
            Some(option) => {
                return Err(CliError::Usage(format!(
                    "unknown song-lua-analyze option `{option}`"
                )));
            }
            None => {
                return Err(CliError::Usage(
                    "song-lua-analyze option is not valid UTF-8".into(),
                ));
            }
        }
    }
    Ok(SongLuaAnalysisArgs { trace, out })
}

fn emit_song_lua_analysis(args: SongLuaAnalysisArgs) -> Result<(), CliError> {
    let bytes = std::fs::read(&args.trace).map_err(|source| CliError::InputFile {
        path: args.trace.clone(),
        source,
    })?;
    let mut document = serde_json::from_slice(&bytes).map_err(|source| CliError::InputJson {
        path: args.trace,
        source,
    })?;
    song_lua_semantics::enrich(&mut document).map_err(CliError::SongLuaSemantics)?;
    emit_json_document(&document, args.out)
}

fn emit_json_document(document: &serde_json::Value, out: Option<PathBuf>) -> Result<(), CliError> {
    if let Some(path) = out {
        let file =
            std::fs::File::create(&path).map_err(|source| CliError::OutputFile { path, source })?;
        serde_json::to_writer_pretty(file, document).map_err(CliError::Output)?;
    } else {
        serde_json::to_writer_pretty(std::io::stdout().lock(), document)
            .map_err(CliError::Output)?;
        println!();
    }
    Ok(())
}

struct SongLuaArgs {
    simfile: PathBuf,
}

fn parse_song_lua_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<SongLuaArgs, CliError> {
    let simfile = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("song-lua command requires a simfile".into()))?;
    if let Some(extra) = args.next() {
        return Err(CliError::Usage(format!(
            "unexpected song-lua argument `{}`",
            extra.to_string_lossy()
        )));
    }
    Ok(SongLuaArgs { simfile })
}

fn emit_song_lua(args: SongLuaArgs) -> Result<(), CliError> {
    let document = song_lua_oracle::load(&args.simfile).map_err(CliError::SongLua)?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &document).map_err(CliError::Output)?;
    println!();
    Ok(())
}

struct SongLuaBaselineArgs {
    songs_root: PathBuf,
    output_dir: PathBuf,
}

fn parse_song_lua_baseline_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<SongLuaBaselineArgs, CliError> {
    let songs_root = args.next().map(PathBuf::from).ok_or_else(|| {
        CliError::Usage("song-lua-baseline command requires a songs directory".into())
    })?;
    let mut output_dir = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out") => {
                let path = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| CliError::Usage("--out requires a path".into()))?;
                set_once(&mut output_dir, path, "--out")?;
            }
            Some(option) => {
                return Err(CliError::Usage(format!(
                    "unknown song-lua-baseline option `{option}`"
                )));
            }
            None => {
                return Err(CliError::Usage(
                    "song-lua-baseline option is not valid UTF-8".into(),
                ));
            }
        }
    }
    Ok(SongLuaBaselineArgs {
        songs_root,
        output_dir: output_dir
            .ok_or_else(|| CliError::Usage("song-lua-baseline requires --out".into()))?,
    })
}

fn run_song_lua_baseline(args: SongLuaBaselineArgs) -> Result<(), CliError> {
    let report = song_lua_baseline::generate(&args.songs_root, &args.output_dir)
        .map_err(CliError::SongLuaBaseline)?;
    println!(
        "wrote {} song Lua simfile fixtures with {} referenced Lua entries to {}",
        report.simfiles,
        report.lua_entries,
        args.output_dir.display()
    );
    Ok(())
}

struct SongLuaSemanticBaselineArgs {
    songs_root: PathBuf,
    output_dir: PathBuf,
    beat_step: f32,
    max_events: u32,
    difficulty_code: i32,
    steps_type: Option<String>,
    random_seed: u32,
    until_beat: Option<f32>,
    simfiles: Vec<PathBuf>,
}

fn parse_song_lua_semantic_baseline_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<SongLuaSemanticBaselineArgs, CliError> {
    let songs_root = args.next().map(PathBuf::from).ok_or_else(|| {
        CliError::Usage("song-lua-semantic-baseline requires a songs directory".into())
    })?;
    let mut output_dir = None;
    let mut beat_step = None;
    let mut max_events = None;
    let mut difficulty_code = None;
    let mut steps_type = None;
    let mut random_seed = None;
    let mut until_beat = None;
    let mut simfiles = Vec::new();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out") => set_once(
                &mut output_dir,
                PathBuf::from(next_utf8(&mut args, "--out")?),
                "--out",
            )?,
            Some("--difficulty") => {
                let raw = next_utf8(&mut args, "--difficulty")?;
                let value = song_lua_semantic_baseline::difficulty_code(&raw).ok_or_else(|| {
                    CliError::Usage(format!(
                        "difficulty `{raw}` is not Beginner, Easy, Medium, Hard, Challenge, or Edit"
                    ))
                })?;
                set_once(&mut difficulty_code, value, "--difficulty")?;
            }
            Some("--steps-type") => set_once(
                &mut steps_type,
                next_utf8(&mut args, "--steps-type")?,
                "--steps-type",
            )?,
            Some("--random-seed") => {
                let raw = next_utf8(&mut args, "--random-seed")?;
                let value = raw.parse::<u32>().map_err(|_| {
                    CliError::Usage(format!("random seed `{raw}` is not an integer"))
                })?;
                set_once(&mut random_seed, value, "--random-seed")?;
            }
            Some("--beat-step") => {
                let raw = next_utf8(&mut args, "--beat-step")?;
                let value = raw.parse::<f32>().map_err(|_| {
                    CliError::Usage(format!("semantic beat step `{raw}` is not a number"))
                })?;
                set_once(&mut beat_step, value, "--beat-step")?;
            }
            Some("--max-events") => {
                let raw = next_utf8(&mut args, "--max-events")?;
                let value = raw.parse::<u32>().map_err(|_| {
                    CliError::Usage(format!("semantic event limit `{raw}` is not an integer"))
                })?;
                set_once(&mut max_events, value, "--max-events")?;
            }
            Some("--simfile") => simfiles.push(PathBuf::from(next_utf8(&mut args, "--simfile")?)),
            Some("--until-beat") => {
                let raw = next_utf8(&mut args, "--until-beat")?;
                let value = raw.parse::<f32>().map_err(|_| {
                    CliError::Usage(format!("semantic endpoint `{raw}` is not a number"))
                })?;
                set_once(&mut until_beat, value, "--until-beat")?;
            }
            Some(option) => {
                return Err(CliError::Usage(format!(
                    "unknown song-lua-semantic-baseline option `{option}`"
                )));
            }
            None => {
                return Err(CliError::Usage(
                    "song-lua-semantic-baseline option is not valid UTF-8".into(),
                ));
            }
        }
    }
    Ok(SongLuaSemanticBaselineArgs {
        songs_root,
        output_dir: output_dir
            .ok_or_else(|| CliError::Usage("song-lua-semantic-baseline requires --out".into()))?,
        beat_step: beat_step.unwrap_or(0.25),
        max_events: max_events.unwrap_or(500_000),
        difficulty_code: difficulty_code.unwrap_or(song_lua_semantic_baseline::DEFAULT_DIFFICULTY),
        steps_type,
        random_seed: random_seed.unwrap_or(1),
        until_beat,
        simfiles,
    })
}

fn run_song_lua_semantic_baseline(args: SongLuaSemanticBaselineArgs) -> Result<(), CliError> {
    let report = song_lua_semantic_baseline::generate(
        &args.songs_root,
        &args.output_dir,
        args.beat_step,
        args.max_events,
        args.difficulty_code,
        args.steps_type.as_deref(),
        args.random_seed,
        args.until_beat,
        &args.simfiles,
    )
    .map_err(CliError::SongLuaSemanticBaseline)?;
    println!(
        "generated {} complete and {} partial semantic song Lua fixtures of {} from {} unique sessions ({} failed); manifest: {}",
        report.complete,
        report.partial,
        report.simfiles,
        report.evaluated_sessions,
        report.failed,
        report.manifest.display()
    );
    Ok(())
}

struct SongLuaArchiveArgs {
    songs_root: PathBuf,
    trace_root: PathBuf,
    output_dir: PathBuf,
}

fn parse_song_lua_archive_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<SongLuaArchiveArgs, CliError> {
    let songs_root = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("song-lua-archive requires a songs directory".into()))?;
    let mut trace_root = None;
    let mut output_dir = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--traces") => set_once(
                &mut trace_root,
                PathBuf::from(next_utf8(&mut args, "--traces")?),
                "--traces",
            )?,
            Some("--out") => set_once(
                &mut output_dir,
                PathBuf::from(next_utf8(&mut args, "--out")?),
                "--out",
            )?,
            Some(option) => {
                return Err(CliError::Usage(format!(
                    "unknown song-lua-archive option `{option}`"
                )));
            }
            None => {
                return Err(CliError::Usage(
                    "song-lua-archive option is not valid UTF-8".into(),
                ));
            }
        }
    }
    Ok(SongLuaArchiveArgs {
        songs_root,
        trace_root: trace_root
            .ok_or_else(|| CliError::Usage("song-lua-archive requires --traces".into()))?,
        output_dir: output_dir
            .ok_or_else(|| CliError::Usage("song-lua-archive requires --out".into()))?,
    })
}

fn run_song_lua_archive(args: SongLuaArchiveArgs) -> Result<(), CliError> {
    let report = song_lua_archive::generate(&args.songs_root, &args.trace_root, &args.output_dir)
        .map_err(CliError::SongLuaArchive)?;
    println!(
        "wrote {} content-addressed song archives ({} source bytes to {} compressed bytes); index: {}",
        report.archives,
        report.source_bytes,
        report.archive_bytes,
        report.index.display()
    );
    Ok(())
}

struct NoteSkinArgs {
    root: PathBuf,
    game: String,
    skin: String,
}

fn parse_noteskin_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<NoteSkinArgs, CliError> {
    let root = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("noteskin command requires a NoteSkins directory".into()))?;
    let game = next_utf8(&mut args, "noteskin game")?;
    let skin = next_utf8(&mut args, "noteskin name")?;
    if let Some(extra) = args.next() {
        return Err(CliError::Usage(format!(
            "unexpected noteskin argument `{}`",
            extra.to_string_lossy()
        )));
    }
    Ok(NoteSkinArgs { root, game, skin })
}

fn emit_noteskin(args: NoteSkinArgs) -> Result<(), CliError> {
    let document = noteskin_baseline::probe(&args.root, &args.game, &args.skin)
        .map_err(CliError::NoteSkinBaseline)?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &document).map_err(CliError::Output)?;
    println!();
    Ok(())
}

struct NoteSkinBaselineArgs {
    root: PathBuf,
    output_dir: PathBuf,
}

fn parse_noteskin_baseline_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<NoteSkinBaselineArgs, CliError> {
    let root = args.next().map(PathBuf::from).ok_or_else(|| {
        CliError::Usage("noteskin-baseline command requires a NoteSkins directory".into())
    })?;
    let mut output_dir = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out") => {
                let path = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| CliError::Usage("--out requires a path".into()))?;
                set_once(&mut output_dir, path, "--out")?;
            }
            Some(option) => {
                return Err(CliError::Usage(format!(
                    "unknown noteskin-baseline option `{option}`"
                )));
            }
            None => {
                return Err(CliError::Usage(
                    "noteskin-baseline option is not valid UTF-8".into(),
                ));
            }
        }
    }
    Ok(NoteSkinBaselineArgs {
        root,
        output_dir: output_dir
            .ok_or_else(|| CliError::Usage("noteskin-baseline command requires --out".into()))?,
    })
}

fn run_noteskin_baseline(args: NoteSkinBaselineArgs) -> Result<(), CliError> {
    let report = noteskin_baseline::generate(&args.root, &args.output_dir)
        .map_err(CliError::NoteSkinBaseline)?;
    println!(
        "wrote {} noteskin fixtures with {} metrics and {} paths to {} ({} ITGmania diagnostics)",
        report.skins,
        report.metrics,
        report.paths,
        args.output_dir.display(),
        report.diagnostics
    );
    Ok(())
}

struct FontArgs {
    path: PathBuf,
    text: String,
}

fn parse_font_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<FontArgs, CliError> {
    let path = args.next().map(PathBuf::from).ok_or_else(|| {
        CliError::Usage("font command requires a font .ini or .redir path".into())
    })?;
    let mut text = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--text") => set_once(&mut text, next_utf8(&mut args, "--text")?, "--text")?,
            Some(option) => {
                return Err(CliError::Usage(format!("unknown font option `{option}`")));
            }
            None => return Err(CliError::Usage("font option is not valid UTF-8".into())),
        }
    }
    Ok(FontArgs {
        path,
        text: text.unwrap_or_else(|| DEFAULT_FONT_TEXT.into()),
    })
}

fn emit_font(args: FontArgs) -> Result<(), CliError> {
    let document = font_oracle::load(&args.path, &args.text).map_err(CliError::Font)?;
    serde_json::to_writer_pretty(std::io::stdout().lock(), &document).map_err(CliError::Output)?;
    println!();
    Ok(())
}

struct FontBaselineArgs {
    font_root: PathBuf,
    output_dir: PathBuf,
}

fn parse_font_baseline_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<FontBaselineArgs, CliError> {
    let font_root = args.next().map(PathBuf::from).ok_or_else(|| {
        CliError::Usage("font-baseline command requires a Fonts directory".into())
    })?;
    let mut output_dir = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--out") => {
                let path = args
                    .next()
                    .map(PathBuf::from)
                    .ok_or_else(|| CliError::Usage("--out requires a path".into()))?;
                set_once(&mut output_dir, path, "--out")?;
            }
            Some(option) => {
                return Err(CliError::Usage(format!(
                    "unknown font-baseline option `{option}`"
                )));
            }
            None => {
                return Err(CliError::Usage(
                    "font-baseline option is not valid UTF-8".into(),
                ));
            }
        }
    }
    Ok(FontBaselineArgs {
        font_root,
        output_dir: output_dir
            .ok_or_else(|| CliError::Usage("font-baseline command requires --out".into()))?,
    })
}

fn run_font_baseline(args: FontBaselineArgs) -> Result<(), CliError> {
    let report = font_baseline::generate(&args.font_root, &args.output_dir)
        .map_err(CliError::FontBaseline)?;
    println!(
        "wrote {} font fixtures to {} ({} ITGmania diagnostics)",
        report.fonts,
        args.output_dir.display(),
        report.diagnostics
    );
    Ok(())
}

struct ManifestDirArgs {
    manifest: PathBuf,
    directory: PathBuf,
}

fn parse_manifest_dir_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
    command: &str,
    directory_option: &str,
) -> Result<ManifestDirArgs, CliError> {
    let manifest = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage(format!("{command} command requires a manifest")))?;
    let mut directory = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some(option) if option == directory_option => {
                let path = args.next().map(PathBuf::from).ok_or_else(|| {
                    CliError::Usage(format!("{directory_option} requires a path"))
                })?;
                set_once(&mut directory, path, directory_option)?;
            }
            Some(option) => {
                return Err(CliError::Usage(format!(
                    "unknown {command} option `{option}`"
                )));
            }
            None => {
                return Err(CliError::Usage(format!(
                    "{command} option is not valid UTF-8"
                )));
            }
        }
    }
    Ok(ManifestDirArgs {
        manifest,
        directory: directory.ok_or_else(|| {
            CliError::Usage(format!("{command} command requires {directory_option}"))
        })?,
    })
}

fn run_baseline(args: ManifestDirArgs) -> Result<(), CliError> {
    let count = baseline::generate(&args.manifest, &args.directory).map_err(CliError::Baseline)?;
    println!(
        "wrote {count} baselines and provenance to {}",
        args.directory.display()
    );
    Ok(())
}

fn run_verify(args: ManifestDirArgs) -> Result<(), CliError> {
    let count = baseline::verify(&args.manifest, &args.directory).map_err(CliError::Baseline)?;
    println!(
        "verified {count} baselines against {}",
        args.directory.display()
    );
    Ok(())
}

struct DiffArgs {
    expected: PathBuf,
    actual: PathBuf,
}

fn parse_diff_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<DiffArgs, CliError> {
    let expected = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("diff command requires EXPECTED and ACTUAL files".into()))?;
    let actual = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("diff command requires an ACTUAL file".into()))?;
    if let Some(extra) = args.next() {
        return Err(CliError::Usage(format!(
            "unexpected diff argument `{}`",
            extra.to_string_lossy()
        )));
    }
    Ok(DiffArgs { expected, actual })
}

fn run_diff(args: DiffArgs) -> Result<(), CliError> {
    let expected = json_diff::read(&args.expected).map_err(CliError::Json)?;
    let actual = json_diff::read(&args.actual).map_err(CliError::Json)?;
    let differences = json_diff::compare(&expected, &actual);
    if differences.is_empty() {
        println!("no differences");
        Ok(())
    } else {
        Err(CliError::Differences(differences))
    }
}

struct ChartArgs {
    simfile: PathBuf,
    theme: Option<PathBuf>,
    output: Option<PathBuf>,
    rssp_baseline: Option<PathBuf>,
    theme_repairs: Option<PathBuf>,
    select_one: bool,
    selector: selector::Selector,
}

fn parse_chart_args(
    args: impl Iterator<Item = std::ffi::OsString>,
    one_chart: bool,
) -> Result<ChartArgs, CliError> {
    let mut args = args.peekable();
    let simfile = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| CliError::Usage("chart command requires a simfile".into()))?;
    let mut index = None;
    let mut steps_type = None;
    let mut difficulty_code = None;
    let mut description = None;
    let mut theme = None;
    let mut output = None;
    let mut rssp_baseline = None;
    let mut theme_repairs = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--rssp-baseline") => {
                let value = args.next().ok_or_else(|| {
                    CliError::Usage("--rssp-baseline requires a directory".into())
                })?;
                set_once(&mut rssp_baseline, PathBuf::from(value), "--rssp-baseline")?;
            }
            Some("--theme-repairs") => {
                let value = args
                    .next()
                    .ok_or_else(|| CliError::Usage("--theme-repairs requires a manifest".into()))?;
                set_once(&mut theme_repairs, PathBuf::from(value), "--theme-repairs")?;
            }
            Some("--out") => {
                let value = args
                    .next()
                    .ok_or_else(|| CliError::Usage("--out requires a path".into()))?;
                set_once(&mut output, PathBuf::from(value), "--out")?;
            }
            Some("--theme") => {
                let value = args
                    .next()
                    .ok_or_else(|| CliError::Usage("--theme requires a path".into()))?;
                set_once(&mut theme, PathBuf::from(value), "--theme")?;
            }
            Some("--index") if one_chart => {
                let value = args
                    .next()
                    .ok_or_else(|| CliError::Usage("--index requires a value".into()))?;
                let value = value
                    .to_str()
                    .ok_or_else(|| CliError::Usage("chart index is not valid UTF-8".into()))?;
                let parsed = value.parse().map_err(|_| {
                    CliError::Usage(format!(
                        "chart index `{value}` is not a non-negative integer"
                    ))
                })?;
                if index.replace(parsed).is_some() {
                    return Err(CliError::Usage("--index may only be provided once".into()));
                }
            }
            Some("--steps-type") if one_chart => {
                set_once(
                    &mut steps_type,
                    next_utf8(&mut args, "--steps-type")?,
                    "--steps-type",
                )?;
            }
            Some("--difficulty-code") if one_chart => {
                let value = next_utf8(&mut args, "--difficulty-code")?;
                let parsed = value.parse().map_err(|_| {
                    CliError::Usage(format!("difficulty code `{value}` is not an integer"))
                })?;
                set_once(&mut difficulty_code, parsed, "--difficulty-code")?;
            }
            Some("--description") if one_chart => {
                set_once(
                    &mut description,
                    next_utf8(&mut args, "--description")?,
                    "--description",
                )?;
            }
            Some(option) => {
                return Err(CliError::Usage(format!("unknown chart option `{option}`")));
            }
            None => return Err(CliError::Usage("chart option is not valid UTF-8".into())),
        }
    }
    let selector = selector::Selector {
        index,
        steps_type,
        difficulty_code,
        description,
    };
    selector.validate().map_err(CliError::Selector)?;
    if (rssp_baseline.is_some() || theme_repairs.is_some()) && theme.is_none() {
        return Err(CliError::Usage(
            "RSSP output and theme repairs require --theme".into(),
        ));
    }
    Ok(ChartArgs {
        simfile,
        theme,
        output,
        rssp_baseline,
        theme_repairs,
        select_one: one_chart,
        selector,
    })
}

fn next_utf8(
    args: &mut impl Iterator<Item = std::ffi::OsString>,
    option: &str,
) -> Result<String, CliError> {
    args.next()
        .ok_or_else(|| CliError::Usage(format!("{option} requires a value")))?
        .into_string()
        .map_err(|_| CliError::Usage(format!("{option} value is not valid UTF-8")))
}

fn set_once<T>(slot: &mut Option<T>, value: T, option: &str) -> Result<(), CliError> {
    if slot.replace(value).is_some() {
        return Err(CliError::Usage(format!(
            "{option} may only be provided once"
        )));
    }
    Ok(())
}

fn emit_charts(args: ChartArgs) -> Result<(), CliError> {
    let mut document =
        oracle::load_with_theme(&args.simfile, args.theme.as_deref()).map_err(CliError::Oracle)?;
    if let Some(manifest) = args.theme_repairs {
        let theme = args
            .theme
            .as_deref()
            .ok_or_else(|| CliError::Usage("theme repairs require --theme".into()))?;
        oracle::apply_theme_repairs(&mut document, &manifest, theme).map_err(CliError::Oracle)?;
    }
    if args.select_one {
        let index = args
            .selector
            .select(&document.charts)
            .map_err(CliError::Selector)?;
        document.charts = vec![document.charts.swap_remove(index)];
    }
    let mut rssp_error = None;
    let mut source_md5 = None;
    if let Some(root) = args.rssp_baseline {
        let source = oracle::source_bytes(&args.simfile).map_err(CliError::Oracle)?;
        let digest = format!("{:x}", md5::compute(&source));
        let dir = root.join(&digest[..2]);
        let target = dir.join(format!("{digest}.json.zst"));
        match chart_report::project(&document, &source) {
            Ok(report) => {
                let bytes = serde_json::to_vec(&report).map_err(CliError::Output)?;
                let bytes = zstd::encode_all(bytes.as_slice(), 3).map_err(|source| {
                    CliError::OutputFile {
                        path: dir.clone(),
                        source,
                    }
                })?;
                std::fs::create_dir_all(&dir).map_err(|source| CliError::OutputFile {
                    path: dir.clone(),
                    source,
                })?;
                let pending = dir.join(format!("{digest}.{}.pending", std::process::id()));
                std::fs::write(&pending, bytes).map_err(|source| CliError::OutputFile {
                    path: pending.clone(),
                    source,
                })?;
                std::fs::rename(&pending, &target).map_err(|source| CliError::OutputFile {
                    path: target,
                    source,
                })?;
            }
            Err(error) => {
                // An incomplete recapture must not leave a stale report that
                // an existing parity consumer could mistake for this run.
                match std::fs::remove_file(&target) {
                    Ok(()) => {}
                    Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(CliError::OutputFile {
                            path: target,
                            source,
                        });
                    }
                }
                rssp_error = Some(error);
            }
        }
        source_md5 = Some(digest);
    }
    if let Some(path) = args.output {
        let bytes = serde_json::to_vec(&document).map_err(CliError::Output)?;
        let compressed = path.extension().is_some_and(|ext| ext == "zst");
        let bytes = if compressed {
            zstd::encode_all(bytes.as_slice(), 3).map_err(|source| CliError::OutputFile {
                path: path.clone(),
                source,
            })?
        } else {
            bytes
        };
        std::fs::write(&path, bytes).map_err(|source| CliError::OutputFile {
            path: path.clone(),
            source,
        })?;
        let partial: Vec<_> = document.charts.iter().filter_map(|chart| {
            if !chart.note_data_supported { return None; }
            chart.theme.as_ref().filter(|theme| theme["status"] != "complete")
                .map(|theme| serde_json::json!({"index": chart.source_index, "status": theme["status"], "errors": theme["errors"]}))
        }).collect();
        serde_json::to_writer(
            std::io::stdout().lock(),
            &serde_json::json!({
                "source_sha256": document.source_sha256, "charts": document.charts.len(),
                "source_md5": source_md5, "rssp_error": rssp_error,
                "theme_repairs": document.theme_repairs,
                "diagnostics": document.diagnostics, "partial_theme_charts": partial,
                "unsupported_charts": document.charts.iter().filter(|chart| !chart.note_data_supported)
                    .map(|chart| serde_json::json!({"index": chart.source_index, "steps_type": chart.steps_type.source}))
                    .collect::<Vec<_>>(),
                "raw_metadata_charts": document.charts.iter().filter(|chart| !chart.metadata_utf8)
                    .map(|chart| chart.source_index).collect::<Vec<_>>()
            }),
        )
        .map_err(CliError::Output)?;
    } else {
        if let Some(error) = rssp_error {
            return Err(CliError::Usage(error));
        }
        serde_json::to_writer_pretty(std::io::stdout().lock(), &document)
            .map_err(CliError::Output)?;
    }
    println!();
    Ok(())
}

fn parse_doctor_args(
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<Option<PathBuf>, CliError> {
    let mut root = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--itgmania-root") => {
                let path = args
                    .next()
                    .ok_or_else(|| CliError::Usage("--itgmania-root requires a path".into()))?;
                if root.replace(PathBuf::from(path)).is_some() {
                    return Err(CliError::Usage(
                        "--itgmania-root may only be provided once".into(),
                    ));
                }
            }
            Some(option) => {
                return Err(CliError::Usage(format!("unknown doctor option `{option}`")));
            }
            None => return Err(CliError::Usage("path is not valid UTF-8".into())),
        }
    }
    Ok(root)
}

fn doctor(cli_root: Option<PathBuf>) -> Result<(), CliError> {
    let root = resolve_root(cli_root)?;
    println!("ITGmania root: {}", root.display());

    let mut missing = Vec::new();
    for &(label, relative) in CORE_FILES {
        if root.join(relative).is_file() {
            println!("  ok  {label}: {relative}");
        } else {
            println!("  missing  {label}: {relative}");
            missing.push(relative);
        }
    }

    match find_theme_scripts(&root) {
        Some((parser, helpers)) => {
            println!("  ok  optional theme parser: {}", parser.display());
            println!("  ok  optional theme helpers: {}", helpers.display());
        }
        None => println!("  note  optional Simply Love chart parser was not found"),
    }

    #[cfg(itgmania_oracle)]
    println!("  ok  native ITGmania oracle is compiled into this binary");
    #[cfg(not(itgmania_oracle))]
    println!("  note  native oracle is not compiled for this platform");

    if missing.is_empty() {
        println!("status: ITGmania source tree is usable");
        Ok(())
    } else {
        Err(CliError::InvalidTree {
            root,
            missing: missing.join(", "),
        })
    }
}

fn resolve_root(cli_root: Option<PathBuf>) -> Result<PathBuf, CliError> {
    let root = cli_root
        .or_else(|| env::var_os("ITGMANIA_ROOT").map(PathBuf::from))
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/itgmania"));

    root.canonicalize()
        .map_err(|source| CliError::RootIo { root, source })
}

fn find_theme_scripts(root: &Path) -> Option<(PathBuf, PathBuf)> {
    THEME_SCRIPT_PAIRS
        .iter()
        .map(|(parser, helpers)| (root.join(parser), root.join(helpers)))
        .chain(std::iter::once({
            let theme = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/simply-love/Scripts");
            (
                theme.join("SL-ChartParser.lua"),
                theme.join("SL-ChartParserHelpers.lua"),
            )
        }))
        .find(|(parser, helpers)| parser.is_file() && helpers.is_file())
}

fn print_help() {
    println!(
        "itgmania-harness-rs {VERSION}\n\
         Generate reference values through ITGmania internals.\n\n\
         Usage:\n\
           itgmania-harness-rs doctor [--itgmania-root PATH]\n\
           itgmania-harness-rs actor-conformance FIXTURE.json [--out FILE]\n\
           itgmania-harness-rs charts SIMFILE [--theme PATH] [--out FILE]\n\
             [--rssp-baseline DIR] [--theme-repairs MANIFEST.json]\n\
           itgmania-harness-rs chart SIMFILE [--index INDEX]\n\
             [--steps-type SOURCE] [--difficulty-code CODE]\n\
             [--description TEXT] [--theme PATH]\n\
           itgmania-harness-rs font FONT.ini [--text TEXT]\n\
           itgmania-harness-rs font-baseline FONTS_DIR --out DIR\n\
           itgmania-harness-rs noteskin NOTESKINS_DIR GAME SKIN\n\
           itgmania-harness-rs noteskin-baseline NOTESKINS_DIR --out DIR\n\
           itgmania-harness-rs song-lua SIMFILE\n\
           itgmania-harness-rs song-lua-baseline SONGS_DIR --out DIR\n\
           itgmania-harness-rs song-lua-semantic-baseline SONGS_DIR --out DIR\n\
             [--difficulty Challenge] [--steps-type SOURCE]\n\
             [--beat-step BEATS] [--max-events COUNT] [--random-seed 1]\n\
             [--until-beat MINIMUM] [--simfile RELATIVE_PATH ...]\n\
           itgmania-harness-rs song-lua-archive SONGS_DIR --traces DIR --out DIR\n\
           itgmania-harness-rs song-lua-trace SIMFILE --itgmania-bin PATH\n\
             [--until-beat 16] [--style single] [--difficulty Challenge]\n\
             [--fallback-theme Simply-Love-SM5] [--timeout SECONDS] [--out FILE]\n\
             [--keep-run-dir]\n\
           itgmania-harness-rs song-lua-analyze TRACE [--out FILE]\n\
           itgmania-harness-rs baseline MANIFEST --out DIR\n\
           itgmania-harness-rs verify MANIFEST --against DIR\n\
           itgmania-harness-rs diff EXPECTED ACTUAL\n\
           itgmania-harness-rs --version\n\n\
         Environment:\n\
           ITGMANIA_ROOT  ITGmania source tree (default: vendor/itgmania)\n\n\
         Native chart, font, noteskin, and song-Lua semantic builds on Windows and Linux/WSL."
    );
}

#[derive(Debug)]
enum CliError {
    Usage(String),
    RootIo {
        root: PathBuf,
        source: std::io::Error,
    },
    InvalidTree {
        root: PathBuf,
        missing: String,
    },
    Oracle(oracle::Error),
    ActorConformance(actor_conformance::Error),
    Font(font_oracle::Error),
    FontBaseline(font_baseline::Error),
    NoteSkinBaseline(noteskin_baseline::Error),
    SongLua(song_lua_oracle::Error),
    SongLuaBaseline(song_lua_baseline::Error),
    SongLuaArchive(song_lua_archive::Error),
    SongLuaSemanticBaseline(song_lua_semantic_baseline::Error),
    SongLuaRuntime(song_lua_runtime::Error),
    SongLuaSemantics(song_lua_semantics::Error),
    InputFile {
        path: PathBuf,
        source: std::io::Error,
    },
    InputJson {
        path: PathBuf,
        source: serde_json::Error,
    },
    Output(serde_json::Error),
    OutputFile {
        path: PathBuf,
        source: std::io::Error,
    },
    Selector(selector::Error),
    Baseline(baseline::Error),
    Json(json_diff::Error),
    Differences(Vec<json_diff::Difference>),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(formatter, "{message}; run with --help for usage"),
            Self::RootIo { root, source } => {
                write!(
                    formatter,
                    "cannot open ITGmania root {}: {source}; initialize the pinned sources \
                     with `git submodule update --init`, or set ITGMANIA_ROOT to a source checkout",
                    root.display()
                )
            }
            Self::InvalidTree { root, missing } => write!(
                formatter,
                "{} is not a usable ITGmania source tree; missing {missing}; \
                 initialize the pinned sources with `git submodule update --init`, \
                 or set ITGMANIA_ROOT to a complete source checkout",
                root.display()
            ),
            Self::Oracle(error) => error.fmt(formatter),
            Self::ActorConformance(error) => error.fmt(formatter),
            Self::Font(error) => error.fmt(formatter),
            Self::FontBaseline(error) => error.fmt(formatter),
            Self::NoteSkinBaseline(error) => error.fmt(formatter),
            Self::SongLua(error) => error.fmt(formatter),
            Self::SongLuaBaseline(error) => error.fmt(formatter),
            Self::SongLuaArchive(error) => error.fmt(formatter),
            Self::SongLuaSemanticBaseline(error) => error.fmt(formatter),
            Self::SongLuaRuntime(error) => error.fmt(formatter),
            Self::SongLuaSemantics(error) => error.fmt(formatter),
            Self::InputFile { path, source } => {
                write!(formatter, "could not read {}: {source}", path.display())
            }
            Self::InputJson { path, source } => {
                write!(formatter, "invalid JSON in {}: {source}", path.display())
            }
            Self::Output(error) => write!(formatter, "could not write JSON output: {error}"),
            Self::OutputFile { path, source } => {
                write!(
                    formatter,
                    "could not create output file {}: {source}",
                    path.display()
                )
            }
            Self::Selector(error) => write!(formatter, "{error}; run with --help for usage"),
            Self::Baseline(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
            Self::Differences(differences) => {
                write!(formatter, "JSON values differ:")?;
                for difference in differences {
                    write!(formatter, "\n  {difference}")?;
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn explicit_root_wins_over_default() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            resolve_root(Some(root.clone())).unwrap(),
            root.canonicalize().unwrap()
        );
    }

    #[test]
    fn doctor_rejects_duplicate_roots() {
        let args = [
            OsString::from("--itgmania-root"),
            OsString::from("one"),
            OsString::from("--itgmania-root"),
            OsString::from("two"),
        ];
        let result = parse_doctor_args(args.into_iter());
        assert!(matches!(result, Err(CliError::Usage(_))));
    }

    #[test]
    fn trace_rejects_negative_end_beat() {
        let args = [
            OsString::from("song.ssc"),
            OsString::from("--itgmania-bin"),
            OsString::from("ITGmania.exe"),
            OsString::from("--until-beat"),
            OsString::from("-1"),
        ];
        assert!(matches!(
            parse_song_lua_trace_args(args.into_iter()),
            Err(CliError::Usage(_))
        ));
    }

    #[test]
    fn finds_checked_in_theme_variant() {
        let root = Path::new(env!("ITGMANIA_BUILD_ROOT"));
        assert!(find_theme_scripts(&root).is_some());
    }

    #[test]
    fn font_text_defaults_and_can_be_overridden() {
        let default = parse_font_args([OsString::from("font.ini")].into_iter()).unwrap();
        assert_eq!(default.text, DEFAULT_FONT_TEXT);

        let custom = parse_font_args(
            [
                OsString::from("font.ini"),
                OsString::from("--text"),
                OsString::from("Aß"),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(custom.text, "Aß");
    }
}
