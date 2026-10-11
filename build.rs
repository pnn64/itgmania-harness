use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const ITG_SOURCES: &[&str] = &[
    "src/EnumHelper.cpp",
    "src/Difficulty.cpp",
    "src/RageUtil.cpp",
    "src/RageUtil/Regex.cpp",
    "src/RageUtil/RandomNumbers.cpp",
    "src/RageException.cpp",
    "src/RageThreads.cpp",
    "src/RageTimer.cpp",
    "src/GameConstantsAndTypes.cpp",
    "src/IniFile.cpp",
    "src/NoteTypes.cpp",
    "src/NoteData.cpp",
    "src/NoteDataUtil.cpp",
    "src/TimingData.cpp",
    "src/SongPosition.cpp",
    "src/TimingSegments.cpp",
    "src/Steps.cpp",
    "src/StepParityGenerator.cpp",
    "src/StepParityCost.cpp",
    "src/StepParityDatastructs.cpp",
    "src/TechCounts.cpp",
    "src/ScreenMessage.cpp",
    "src/MessageManager.cpp",
    "src/PlayerStageStats.cpp",
    "src/PlayerState.cpp",
    "src/SampleHistory.cpp",
    "src/Attack.cpp",
    "src/NoteDataWithScoring.cpp",
    "src/GamePreferences.cpp",
    "src/HighScore.cpp",
    "src/Grade.cpp",
    "src/DateTime.cpp",
    "src/NotesLoader.cpp",
    "src/NotesLoaderSMA.cpp",
    "src/NotesLoaderSM.cpp",
    "src/NotesLoaderSSC.cpp",
    "src/NotesWriterSM.cpp",
    "src/NotesWriterSSC.cpp",
    "src/ColumnCues.cpp",
    "src/CubicSpline.cpp",
    "src/Actor.cpp",
    "src/ActorProxy.cpp",
    "src/ActorFrame.cpp",
    "src/ActorFrameTexture.cpp",
    "src/Tween.cpp",
    "src/RageMath.cpp",
    "src/RageTypes.cpp",
    "src/RageDisplay.cpp",
    "src/RageSurface.cpp",
    "src/RageSurfaceUtils.cpp",
    "src/RageSurfaceUtils_Zoom.cpp",
    "src/RageSurfaceUtils_Dither.cpp",
    "src/RageSurface_Load.cpp",
    "src/RageSurface_Load_PNG.cpp",
    "src/RageSurface_Load_GIF.cpp",
    "src/RageSurface_Load_BMP.cpp",
    "src/RageSurface_Load_JPEG.cpp",
    "src/RageBitmapTexture.cpp",
    "src/Sprite.cpp",
    "src/ModelTypes.cpp",
    "src/ModelManager.cpp",
    "src/RageModelGeometry.cpp",
    "src/Font.cpp",
    "src/FontManager.cpp",
    "src/BitmapText.cpp",
    "src/FontCharAliases.cpp",
    "src/FontCharmaps.cpp",
    "src/MsdFile.cpp",
    "src/MeasureInfo.cpp",
    "src/RageTexture.cpp",
    "src/RageTextureRenderTarget.cpp",
    "src/RageTextureID.cpp",
    "src/RageFileBasic.cpp",
    "src/RageFile.cpp",
    "src/RageSoundReader.cpp",
    "src/RageSoundReader_WAV.cpp",
    "src/RageSoundReader_Vorbisfile.cpp",
    "src/XmlFile.cpp",
    "src/XmlFileUtil.cpp",
    "src/SpecialFiles.cpp",
    "src/NoteSkinManager.cpp",
    "src/global.cpp",
];

const LUA_SOURCES: &[&str] = &[
    "lapi.c",
    "lauxlib.c",
    "lbaselib.c",
    "lcode.c",
    "ldblib.c",
    "ldebug.c",
    "ldo.c",
    "ldump.c",
    "lfunc.c",
    "lgc.c",
    "linit.c",
    "liolib.c",
    "llex.c",
    "lmathlib.c",
    "lmem.c",
    "loadlib.c",
    "lobject.c",
    "lopcodes.c",
    "loslib.c",
    "lparser.c",
    "lstate.c",
    "lstring.c",
    "lstrlib.c",
    "ltable.c",
    "ltablib.c",
    "ltm.c",
    "lundump.c",
    "lvm.c",
    "lzio.c",
];

const PCRE_SOURCES: &[&str] = &[
    "pcre_byte_order.c",
    "pcre_compile.c",
    "pcre_config.c",
    "pcre_dfa_exec.c",
    "pcre_exec.c",
    "pcre_fullinfo.c",
    "pcre_get.c",
    "pcre_globals.c",
    "pcre_jit_compile.c",
    "pcre_maketables.c",
    "pcre_newline.c",
    "pcre_ord2utf8.c",
    "pcre_refcount.c",
    "pcre_string_utils.c",
    "pcre_study.c",
    "pcre_tables.c",
    "pcre_ucd.c",
    "pcre_valid_utf8.c",
    "pcre_version.c",
    "pcre_xclass.c",
];

fn main() {
    println!("cargo:rustc-check-cfg=cfg(itgmania_oracle)");
    println!("cargo:rerun-if-env-changed=ITGMANIA_ROOT");
    println!("cargo:rerun-if-changed=native/oracle_bridge.cpp");
    println!("cargo:rerun-if-changed=native/chart_theme.cpp");
    println!("cargo:rerun-if-changed=native/oracle_bridge.h");
    println!("cargo:rerun-if-changed=native/actor_oracle.cpp");
    println!("cargo:rerun-if-changed=native/options_oracle.cpp");
    println!("cargo:rerun-if-changed=native/scoring_oracle.cpp");
    println!("cargo:rerun-if-changed=native/runtime_stubs.cpp");
    println!("cargo:rerun-if-changed=native/runtime_stubs.h");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    if !matches!(target_os.as_str(), "linux" | "windows") {
        println!("cargo:rustc-env=ITGMANIA_BUILD_ROOT=unavailable");
        println!("cargo:rustc-env=ITGMANIA_GIT_REVISION=unavailable");
        println!("cargo:rustc-env=ITGMANIA_GIT_DIRTY=unknown");
        println!("cargo:rustc-env=ITGMANIA_PRODUCT_VERSION=unavailable");
        return;
    }

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let root = env::var_os("ITGMANIA_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest.join("vendor/itgmania"));
    let root = manifest.join(root);
    require_tree(&root);
    watch_git_metadata(&root);
    println!("cargo:rustc-env=ITGMANIA_BUILD_ROOT={}", root.display());
    println!(
        "cargo:rustc-env=ITGMANIA_GIT_REVISION={}",
        git_output(&root, &["rev-parse", "HEAD"]).unwrap_or_else(|| "unavailable".into())
    );
    let dirty = git_output(
        &root,
        &["status", "--porcelain", "--untracked-files=no", "--", "."],
    )
    .map_or(
        "unknown",
        |output| if output.is_empty() { "false" } else { "true" },
    );
    println!("cargo:rustc-env=ITGMANIA_GIT_DIRTY={dirty}");
    let defs_path = root.join("CMake/SMDefs.cmake");
    println!("cargo:rerun-if-changed={}", defs_path.display());
    let defs = fs::read_to_string(defs_path).expect("ITGmania version definitions");
    let version = ["MAJOR", "MINOR", "PATCH"]
        .map(|part| {
            let prefix = format!("set(SM_VERSION_{part} ");
            defs.lines()
                .find_map(|line| line.trim().strip_prefix(&prefix)?.strip_suffix(')'))
                .expect("native product version component")
        })
        .join(".");
    println!("cargo:rustc-env=ITGMANIA_PRODUCT_VERSION={version}");
    for source in [
        "PlayerOptions.cpp",
        "SongOptions.cpp",
        "ActorMultiVertex.cpp",
        "ActorMultiVertex.h",
    ] {
        println!(
            "cargo:rerun-if-changed={}",
            root.join("src").join(source).display()
        );
    }
    for relative in ITG_SOURCES {
        println!("cargo:rerun-if-changed={}", root.join(relative).display());
    }

    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    write_player_option_methods(&root, &out);
    write_song_stats(&root, &out);
    write_scoring_state(&root, &out);
    write_lua_binding(&root, &out);
    prepare_texture_sources(&root, &out);
    compile_image_deps(&root, &out, &target_os);
    write_compat_headers(&out, &target_os);
    if target_os == "windows" {
        prepare_pcre(&root, &out);
    }

    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .define("ITGMANIA_HARNESS", Some("1"))
        .define("ITGMANIA_HARNESS_SOURCE", Some("1"))
        .define("PCRE_STATIC", None)
        .define("PCRE_EXP_DECL", Some("extern"))
        .define("PCRECPP_EXP_DECL", Some("extern"))
        .define("PCRECPP_EXP_DEFN", Some(""))
        .include(root.join("src"))
        .include(root.join("extern/lua-5.1/src"))
        .include(root.join("extern/jsoncpp"))
        .include(out.join("pcre"))
        .include(root.join("extern/pcre"))
        .include(root.join("extern/miniz"))
        .include(root.join("extern/ogg/include"))
        .include(root.join("extern/vorbis/include"))
        .include(root.join("extern/libpng"))
        .include(root.join("extern/zlib"))
        .include(root.join("extern/libjpeg-turbo/src"))
        .include(out.join("native-jpeg"))
        .include(&out)
        .file(manifest.join("native/oracle_bridge.cpp"))
        .file(manifest.join("native/chart_theme.cpp"))
        .file(manifest.join("native/actor_oracle.cpp"))
        .file(manifest.join("native/options_oracle.cpp"))
        .file(manifest.join("native/scoring_oracle.cpp"))
        .file(manifest.join("native/runtime_stubs.cpp"))
        .file(out.join("lua_binding.cpp"));

    if target_os == "windows" {
        build
            .define("WINDOWS", None)
            .define("WIN32", None)
            .define("NOMINMAX", None)
            .define("ITGMANIA_BUNDLED_LUA", None)
            .define("_CRT_SECURE_NO_WARNINGS", None)
            .flag("/EHsc")
            .flag(format!("/FI{}", out.join("lua_compat.hpp").display()));
    } else {
        build
            .define("UNIX", None)
            .flag("-include")
            .flag(out.join("lua_compat.hpp").to_str().unwrap());
    }

    for relative in ITG_SOURCES {
        build.file(root.join(relative));
    }
    build.compile("itgmania_oracle");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=itgmania_png");
    println!("cargo:rustc-link-lib=static=itgmania_zlib");
    println!(
        "cargo:rustc-link-lib=static={}",
        if target_os == "windows" {
            "jpeg-static"
        } else {
            "jpeg"
        }
    );

    compile_audio_deps(&root, &out);
    compile_bundled_lua(&root, &target_os);

    if target_os == "windows" {
        compile_windows_deps(&root, &out);
        println!("cargo:rustc-link-lib=bcrypt");
    } else {
        for library in ["tomcrypt", "tommath", "pcre", "jsoncpp", "m", "dl"] {
            println!("cargo:rustc-link-lib={library}");
        }
    }
    println!("cargo:rustc-cfg=itgmania_oracle");
}

// Use the pinned codec sources and native methods. Zlib's CMake build renames
// a source header, so compile it directly to keep the reference tree untouched.
fn compile_image_deps(root: &Path, out: &Path, target_os: &str) {
    let png = root.join("extern/libpng");
    let zlib = root.join("extern/zlib");
    let jpeg = root.join("extern/libjpeg-turbo");
    assert!(
        png.join("pngread.c").is_file()
            && zlib.join("inflate.c").is_file()
            && jpeg.join("CMakeLists.txt").is_file(),
        "initialize pinned image dependencies: git -C vendor/itgmania submodule update --init extern/libpng extern/zlib extern/libjpeg-turbo"
    );
    fs::copy(
        png.join("scripts/pnglibconf.h.prebuilt"),
        out.join("pnglibconf.h"),
    )
    .expect("copy pinned PNG configuration");
    let mut library = cc::Build::new();
    library
        .cargo_metadata(false)
        .warnings(false)
        .include(&png)
        .include(&zlib)
        .include(out)
        .define("PNG_INTEL_SSE_OPT", Some("0"))
        .define("PNG_ARM_NEON_OPT", Some("0"))
        .define("_CRT_SECURE_NO_WARNINGS", None);
    for file in [
        "png", "pngerror", "pngget", "pngmem", "pngpread", "pngread", "pngrio", "pngrtran",
        "pngrutil", "pngset", "pngtrans", "pngwio", "pngwrite", "pngwtran", "pngwutil",
    ] {
        let source = png.join(format!("{file}.c"));
        println!("cargo:rerun-if-changed={}", source.display());
        library.file(source);
    }
    library.compile("itgmania_png");
    let mut library = cc::Build::new();
    library
        .cargo_metadata(false)
        .warnings(false)
        .include(&zlib)
        .define("_CRT_SECURE_NO_WARNINGS", None);
    for file in [
        "adler32", "compress", "crc32", "deflate", "gzclose", "gzlib", "gzread", "gzwrite",
        "inflate", "infback", "inftrees", "inffast", "trees", "uncompr", "zutil",
    ] {
        let source = zlib.join(format!("{file}.c"));
        println!("cargo:rerun-if-changed={}", source.display());
        library.file(source);
    }
    library.compile("itgmania_zlib");
    let destination = out.join("native-jpeg");
    let tool = cc::Build::new().get_compiler();
    let mut configure = Command::new("cmake");
    configure
        .envs(tool.env().iter().cloned())
        .args(["-S"])
        .arg(&jpeg)
        .arg("-B")
        .arg(&destination)
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DENABLE_SHARED=OFF")
        .arg("-DENABLE_STATIC=ON")
        .arg("-DWITH_SIMD=OFF")
        .arg("-DWITH_TURBOJPEG=OFF")
        .arg("-DWITH_CRT_DLL=ON");
    if target_os == "windows" {
        configure.args(["-G", "NMake Makefiles"]);
    }
    assert!(
        configure
            .status()
            .expect("configure native JPEG codec")
            .success()
    );
    assert!(
        Command::new("cmake")
            .envs(tool.env().iter().cloned())
            .arg("--build")
            .arg(&destination)
            .args(["--target", "jpeg-static", "--config", "Release"])
            .status()
            .expect("build native JPEG codec")
            .success()
    );
    println!("cargo:rustc-link-search=native={}", destination.display());
    // Emit after the oracle library so Unix static archive ordering resolves
    // the native loaders' codec references.
    println!(
        "cargo:rerun-if-changed={}",
        jpeg.join("CMakeLists.txt").display()
    );
    for item in fs::read_dir(jpeg.join("src")).expect("native JPEG source directory") {
        let source = item.expect("JPEG source entry").path();
        if source.is_file() {
            println!("cargo:rerun-if-changed={}", source.display());
        }
    }
}

fn prepare_texture_sources(root: &Path, out: &Path) {
    let path = root.join("src/PrefsManager.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = fs::read_to_string(path).expect("read native game texture defaults");
    let mut defaults = String::new();
    for (field, name) in [
        ("m_iTextureColorDepth", "game_texture_depth"),
        ("m_iMovieColorDepth", "game_movie_depth"),
        ("m_iMaxTextureResolution", "game_max_tex_size"),
    ] {
        let (_, initializer) = source
            .split_once(&format!("{field}(\""))
            .expect("native preference initializer");
        let (_, value) = initializer
            .split_once(',')
            .expect("native preference value");
        let (value, _) = value
            .split_once(')')
            .expect("end of native preference value");
        let value: u32 = value
            .trim()
            .parse()
            .expect("integer native texture default");
        defaults.push_str(&format!("constexpr int {name} = {value};\n"));
    }
    fs::write(out.join("texture_game_defaults.inc"), defaults)
        .expect("write native game texture defaults");
    let path = root.join("src/RageTextureManager.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = fs::read_to_string(path).expect("read native texture preferences");
    let start = source
        .find("void RageTextureManager::AdjustTextureID(")
        .expect("native texture ID adjustment");
    let end = start
        + source[start..]
            .find("\nbool RageTextureManager::IsTextureRegistered(")
            .expect("end of native texture ID adjustment");
    fs::write(out.join("texture_adjust.inc"), &source[start..end])
        .expect("write native texture ID adjustment");
    let path = root.join("src/RageDisplay_OGL.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = fs::read_to_string(path).expect("read native GL pixel formats");
    let start = source
        .find("static RageDisplay::RagePixelFormatDesc PIXEL_FORMAT_DESC")
        .expect("native GL pixel descriptors");
    let end = start
        + source[start..]
            .find("}};")
            .expect("end of pixel format descriptors")
        + 3;
    fs::write(out.join("texture_formats.inc"), &source[start..end])
        .expect("write native pixel format descriptors");
}

// These are the source lists from the pinned Xiph CMakeLists.txt. Decode and
// length calculations remain in ITGmania's unchanged native sound readers.
fn compile_audio_deps(root: &Path, out: &Path) {
    let ogg = root.join("extern/ogg");
    let vorbis = root.join("extern/vorbis");
    assert!(
        ogg.join("src/framing.c").is_file() && vorbis.join("lib/vorbisfile.c").is_file(),
        "initialize native audio dependencies: git -C vendor/itgmania submodule update --init extern/ogg extern/vorbis"
    );
    let types = out.join("ogg");
    fs::create_dir_all(&types).expect("create Ogg compatibility include");
    fs::write(types.join("config_types.h"), "#include <stdint.h>\ntypedef int16_t ogg_int16_t;\ntypedef uint16_t ogg_uint16_t;\ntypedef int32_t ogg_int32_t;\ntypedef uint32_t ogg_uint32_t;\ntypedef int64_t ogg_int64_t;\ntypedef uint64_t ogg_uint64_t;\n").expect("write fixed-width Ogg types");
    let mut codec = cc::Build::new();
    codec
        .warnings(false)
        .include(out)
        .include(ogg.join("include"))
        .include(vorbis.join("include"))
        .include(vorbis.join("lib"))
        .define("_CRT_SECURE_NO_WARNINGS", None);
    for file in [
        "mdct",
        "smallft",
        "block",
        "envelope",
        "window",
        "lsp",
        "lpc",
        "analysis",
        "synthesis",
        "psy",
        "info",
        "floor1",
        "floor0",
        "res0",
        "mapping0",
        "registry",
        "codebook",
        "sharedbook",
        "lookup",
        "bitrate",
        "vorbisfile",
    ] {
        let source = vorbis.join("lib").join(format!("{file}.c"));
        println!("cargo:rerun-if-changed={}", source.display());
        codec.file(source);
    }
    codec.compile("itgmania_vorbis");
    let mut container = cc::Build::new();
    container
        .warnings(false)
        .include(out)
        .include(ogg.join("include"));
    for file in ["bitwise.c", "framing.c"] {
        let source = ogg.join("src").join(file);
        println!("cargo:rerun-if-changed={}", source.display());
        container.file(source);
    }
    container.compile("itgmania_ogg");
}

fn write_song_stats(root: &Path, out: &Path) {
    let path = root.join("src/Song.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = fs::read_to_string(&path).expect("read native Song.cpp");
    let start = source
        .find("void Song::ReCalculateStepStatsAndLastSecond(")
        .expect("native song stats method");
    let end = source[start..]
        .find("\n// Return whether the song is playable")
        .expect("end of native song stats method")
        + start;
    fs::write(out.join("song_stats.inc"), &source[start..end])
        .expect("write native song stats method");
}

fn write_lua_binding(root: &Path, out: &Path) {
    let path = root.join("src/LuaBinding.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = std::fs::read_to_string(path).expect("native Lua binding source");
    let start = source
        .find("static void GetGlobalTable(Lua* L) {")
        .expect("native userdata table");
    let end = source[start..]
        .find("\n}\n")
        .expect("native userdata table end")
        + start
        + 3;
    // ITGmania has one process-wide VM. The harness has one VM per song, so
    // retain the same table/pointer mapping in that VM's registry instead of
    // retaining a numeric LuaReference from a different or closed registry.
    let table = r#"static void GetGlobalTable(Lua* L) {
  static const char key = 0;
  lua_pushlightuserdata(L, const_cast<char*>(&key));
  lua_rawget(L, LUA_REGISTRYINDEX);
  if (lua_isnil(L, -1)) {
    lua_pop(L, 1);
    lua_newtable(L);
    lua_pushlightuserdata(L, const_cast<char*>(&key));
    lua_pushvalue(L, -2);
    lua_rawset(L, LUA_REGISTRYINDEX);
  }
}
"#;
    let adapted = format!("{}{}{}", &source[..start], table, &source[end..]);
    std::fs::write(out.join("lua_binding.cpp"), adapted).expect("VM-owned native Lua bindings");
}

fn write_scoring_state(root: &Path, out: &Path) {
    let path = root.join("src/LuaManager.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = fs::read_to_string(path).expect("read native Lua state pooling");
    let start = source
        .find("struct Impl {")
        .expect("native Lua pool storage");
    let end = start
        + source[start..]
            .find("#if defined(_MSC_VER)")
            .expect("end of Lua pool storage");
    let methods = source
        .find("Lua* LuaManager::Get() {")
        .expect("native Lua acquisition");
    let methods_end = methods + source[methods..].find("\n/*").expect("end of Lua release");
    fs::write(
        out.join("lua_pool.inc"),
        format!(
            "{}\n#define THREAD_POOL 1\n{}",
            &source[start..end],
            &source[methods..methods_end]
        ),
    )
    .expect("write unchanged Lua pooling");
    let path = root.join("src/PrefsManager.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = fs::read_to_string(path).expect("read native scoring preferences");
    let mut preferences = String::new();
    for (field, kind) in [("m_AllowW1", "AllowW1"), ("m_bMercifulBeginner", "bool")] {
        let (_, args) = source
            .split_once(&format!("{field}("))
            .expect("native scoring preference initializer");
        let (args, _) = args.split_once(')').expect("end of scoring preference");
        preferences.push_str(&format!(
            "new (&prefs_raw->{field}) Preference<{kind}>({args});\n"
        ));
    }
    fs::write(out.join("score_preferences.inc"), preferences).expect("write scoring preferences");
    let path = root.join("src/GameState.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = fs::read_to_string(&path).expect("read native scoring game state");
    let mut methods = String::new();
    for (start, end) in [
        (
            "int GameState::GetNumSidesJoined() const {",
            "bool GameState::SetCompatibleStyle(",
        ),
        (
            "PlayerNumber GameState::GetMasterPlayerNumber() const {",
            "void GameState::SetMasterPlayerNumber(",
        ),
        (
            "int GameState::GetNumStagesMultiplierForSong(",
            "int GameState::GetNumStagesForCurrentSongAndStepsOrCourse(",
        ),
        (
            "bool GameState::ShowW1() const {",
            "static ThemeMetric<bool> PROFILE_RECORD_FEATS(",
        ),
        (
            "bool GameState::SetCompatibleStyle(",
            "int GameState::GetNumHumanPlayers() const {",
        ),
        (
            "void GameState::ForceOtherPlayersToCompatibleSteps(",
            "void GameState::Update(",
        ),
        (
            "Difficulty GameState::GetEasiestStepsDifficulty() const {",
            "Difficulty GameState::GetHardestStepsDifficulty() const {",
        ),
        (
            "PlayerNumber GetNextHumanPlayer(",
            "PlayerNumber GetNextCpuPlayer(",
        ),
    ] {
        let start = source
            .find(start)
            .expect("native game state scoring method");
        let end = start
            + source[start..]
                .find(end)
                .expect("end of native game state method");
        methods.push_str(&source[start..end]);
    }
    fs::write(out.join("game_state_scoring.inc"), methods).expect("write unchanged native methods");
    let mut bindings = String::from("class LunaGameState : public Luna<GameState> {\n public:\n");
    for (start, end) in [
        (
            "  DEFINE_METHOD(\n      IsPlayerEnabled,",
            "  DEFINE_METHOD(\n      GetPlayerDisplayName,",
        ),
        (
            "  static void SetCompatibleStyleOrError(",
            "  static int GetCurrentCourse(",
        ),
        (
            "  static int GetHumanPlayers(",
            "  static int IsAnyHumanPlayerUsingMemoryCard(",
        ),
        (
            "  static bool AreStyleAndPlayModeCompatible(",
            "  static int SetCurrentPlayMode(",
        ),
    ] {
        let start = source.find(start).expect("native chart Lua binding");
        let end = start + source[start..].find(end).expect("end of chart Lua binding");
        bindings.push_str(&source[start..end]);
    }
    for name in [
        "GetMasterPlayerNumber",
        "GetNumPlayersEnabled",
        "GetEasiestStepsDifficulty",
    ] {
        let line = source
            .lines()
            .find(|line| line.contains(&format!("DEFINE_METHOD({name},")))
            .expect("native GameState getter macro");
        bindings.push_str(line);
        bindings.push('\n');
    }
    bindings.push_str("  LunaGameState() {\n");
    for name in [
        "GetCurrentSteps",
        "SetCurrentSteps",
        "GetCurrentStyle",
        "SetCurrentStyle",
        "IsPlayerEnabled",
        "IsHumanPlayer",
        "GetEnabledPlayers",
        "GetHumanPlayers",
        "GetNumPlayersEnabled",
        "GetMasterPlayerNumber",
        "GetEasiestStepsDifficulty",
    ] {
        bindings.push_str(&format!("    ADD_METHOD({name});\n"));
    }
    bindings.push_str("  }\n};\nLUA_REGISTER_CLASS(GameState)\n");
    fs::write(out.join("game_state_lua.inc"), bindings).expect("write unchanged chart Lua methods");
    let mut support = String::new();
    for (file, start, end) in [
        (
            "PrefsManager.cpp",
            "static const char* AllowW1Names[]",
            "static const char* MaybeNames[]",
        ),
        (
            "ScoreKeeper.cpp",
            "ScoreKeeper::ScoreKeeper(",
            "#include \"ScoreKeeperNormal.h\"",
        ),
        (
            "Style.cpp",
            "void Style::GetTransformedNoteDataForStyle(",
            "void Style::StyleInputToGameInput(",
        ),
        (
            "Song.cpp",
            "static Preference<float> g_fLongVerSongSeconds(",
            "static Preference<bool> g_BackUpAllSongSaves(",
        ),
        ("Song.cpp", "bool Song::IsLong() const {", "// lua start"),
        (
            "Player.cpp",
            "ThemeMetric<bool> PENALIZE_TAP_SCORE_NONE(",
            "ThemeMetric<bool> JUDGE_HOLD_NOTES_ON_SAME_ROW_TOGETHER(",
        ),
        (
            "Style.cpp",
            "void Style::GetMinAndMaxColX(",
            "std::string Style::ColToButtonName(",
        ),
        (
            "CommonMetrics.cpp",
            "ThemeMetric<bool> CommonMetrics::AUTO_SET_STYLE(",
            "ThemeMetric<int> CommonMetrics::PERCENT_SCORE_DECIMAL_PLACES(",
        ),
        (
            "GameManager.cpp",
            "void GameManager::GetStylesForGame(",
            "const Game* GameManager::GetGameForStyle(",
        ),
        (
            "GameManager.cpp",
            "void GameManager::GetCompatibleStyles(",
            "void GameManager::GetEnabledGames(",
        ),
        (
            "GameManager.cpp",
            "const Style* GameManager::GameAndStringToStyle(",
            "// lua start",
        ),
    ] {
        let path = root.join("src").join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let source = fs::read_to_string(path).expect("read scoring support");
        let start = source.find(start).expect("native scoring support method");
        let end = start + source[start..].find(end).expect("end of scoring support");
        support.push_str(&source[start..end]);
    }
    println!(
        "cargo:rerun-if-changed={}",
        root.join("src/ScoreKeeperNormal.cpp").display()
    );
    fs::write(out.join("score_support.inc"), support).expect("write unchanged scoring support");
    let path = root.join("src/Style.cpp");
    let source = fs::read_to_string(path).expect("read native Style Lua methods");
    let start = source
        .find("class LunaStyle : public Luna<Style>")
        .expect("native Style Lua class");
    let end = start
        + source[start..]
            .find("  static int GetColumnInfo(")
            .expect("end of Style getters");
    let mut bindings = source[start..end].to_owned();
    bindings.push_str("  LunaStyle() {\n");
    for name in [
        "GetName",
        "GetStyleType",
        "GetStepsType",
        "ColumnsPerPlayer",
        "NeedsZoomOutWith2Players",
        "GetWidth",
        "LockedDifficulty",
    ] {
        bindings.push_str(&format!("    ADD_METHOD({name});\n"));
    }
    bindings.push_str("  }\n};\nLUA_REGISTER_CLASS(Style)\n");
    fs::write(out.join("style_lua.inc"), bindings).expect("write unchanged Style getter bindings");
    write_score_metrics(root, out);
    let path = root.join("src/GameManager.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = fs::read_to_string(path).expect("read native dance game definition");
    let tracks = source
        .find("enum {\n  TRACK_1 = 0,")
        .expect("native track enumeration");
    let tracks_end = tracks
        + source[tracks..]
            .find("\n};")
            .expect("end of track enumeration")
        + 3;
    let dance = source
        .find("static const AutoMappings g_AutoKeyMappings_Dance")
        .expect("native dance definitions");
    let dance_end = source
        .find("static const AutoMappings g_AutoKeyMappings_Pump")
        .expect("end of dance definitions");
    fs::write(
        out.join("dance_score.inc"),
        format!(
            "{}\n{}",
            &source[tracks..tracks_end],
            &source[dance..dance_end]
        ),
    )
    .expect("write unchanged native dance definitions");
}

fn write_score_metrics(root: &Path, out: &Path) {
    let theme = PathBuf::from("vendor/simply-love");
    let init = theme.join("Scripts/SL_Init.lua");
    println!("cargo:rerun-if-changed={}", init.display());
    let source = fs::read_to_string(init).expect("read Simply Love scoring settings");
    let start = source
        .find("\tMetrics = {")
        .expect("Simply Love metrics table")
        + "\tMetrics = ".len();
    let end = start
        + source[start..]
            .find("\n\tExWeights = {")
            .expect("end of metrics table");
    let metrics = source[start..end].trim_end().trim_end_matches(',');
    let mut output = format!(
        "static const char* harness_score_settings = R\"ITGMETRIC({metrics})ITGMETRIC\";\n"
    );
    let mut helpers = String::new();
    for (path, start, end) in [
        (
            root.join("Themes/_fallback/Scripts/03 Gameplay.lua"),
            "function ComboContinue()",
            "function EvalUsesCheckpointsWithJudgments()",
        ),
        (
            theme.join("Scripts/SL-Helpers.lua"),
            "GetComboThreshold = function(",
            "-- FailType is a PlayerOption",
        ),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
        let source = fs::read_to_string(path).expect("read source combo rules");
        let start = source.find(start).expect("source combo function");
        let end = start + source[start..].find(end).expect("end of combo function");
        helpers.push_str(&source[start..end]);
    }
    output.push_str(&format!(
        "static const char* harness_combo_rules = R\"ITGMETRIC({helpers})ITGMETRIC\";\n"
    ));
    let prefs = theme.join("Scripts/99 SL-ThemePrefs.lua");
    println!("cargo:rerun-if-changed={}", prefs.display());
    let prefs = fs::read_to_string(prefs).expect("read Simply Love style preference");
    let (_, style) = prefs
        .split_once("PreferredStyle =")
        .expect("PreferredStyle preference");
    let (_, default) = style
        .split_once("Default = ")
        .expect("PreferredStyle default");
    let (default, _) = default
        .split_once(',')
        .expect("end of PreferredStyle default");
    output.push_str(&format!(
        "static const char* harness_style_default = {default};\n"
    ));
    output.push_str("static const char* harness_score_metrics[][3] = {\n");
    for path in [
        root.join("Themes/_fallback/metrics.ini"),
        theme.join("metrics.ini"),
    ] {
        println!("cargo:rerun-if-changed={}", path.display());
        let mut group = String::new();
        for line in fs::read_to_string(path)
            .expect("read scoring theme metrics")
            .lines()
        {
            let line = line.trim();
            if line.starts_with('[') && line.ends_with(']') {
                group = line[1..line.len() - 1].to_owned();
            } else if ["ScoreKeeperNormal", "PlayerStageStats", "Gameplay"]
                .contains(&group.as_str())
                || (group == "Player" && line.starts_with("PenalizeTapScoreNone="))
                || (group == "Common" && line.starts_with("AutoSetStyle="))
            {
                if let Some((name, value)) = line.split_once('=') {
                    if !name.starts_with('#') && !name.starts_with(';') && !value.is_empty() {
                        output.push_str(&format!(
                            "{{{group:?}, {name:?}, R\"ITGMETRIC({value})ITGMETRIC\"}},\n"
                        ));
                    }
                }
            }
        }
    }
    output.push_str("};\n");
    fs::write(out.join("score_metrics.inc"), output).expect("write source scoring metrics");
}

fn write_player_option_methods(root: &Path, out: &Path) {
    let path = root.join("src/PlayerOptions.cpp");
    println!("cargo:rerun-if-changed={}", path.display());
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()));
    let mut methods = Vec::new();
    let mut bool_methods = Vec::new();
    for line in source.lines().map(str::trim) {
        if let Some((name, _)) = line
            .strip_prefix("BOOL_INTERFACE(")
            .and_then(|line| line.split_once(','))
        {
            bool_methods.push(name.trim());
        }
        if let Some(name) = line
            .strip_prefix("ADD_METHOD(")
            .and_then(|line| line.strip_suffix(");"))
        {
            methods.push(name.to_owned());
        } else if let Some(name) = line
            .strip_prefix("ADD_MULTICOL_METHOD(")
            .and_then(|line| line.strip_suffix(");"))
        {
            methods.extend((1..=16).map(|column| format!("{name}{column}")));
        }
    }
    methods.sort();
    methods.dedup();
    assert!(!methods.is_empty(), "no PlayerOptions Lua methods found");
    let mut lua = String::from("_ITG_PLAYER_OPTION_METHODS = {\n");
    let mut native = String::new();
    for method in methods {
        lua.push_str(&format!("  [\"{method}\"] = true,\n"));
        native.push_str(&format!(
            "{{\"{method}\", &LunaPlayerOptions::{method}}},\n"
        ));
    }
    fs::write(out.join("player_option_methods.inc"), native)
        .expect("could not write native PlayerOptions dispatch");
    lua.push_str("}\n");
    lua.push_str("_ITG_PLAYER_OPTION_BOOLS = {\n");
    for method in bool_methods {
        lua.push_str(&format!("  [\"{method}\"] = true,\n"));
    }
    lua.push_str("}\n");
    let mut floats = vec!["XMod".to_owned(), "CMod".to_owned(), "MMod".to_owned()];
    for (token, columns) in [
        ("  FLOAT_INTERFACE(", false),
        ("  FLOAT_NO_SPEED_INTERFACE(", false),
        ("  MULTICOL_FLOAT_INTERFACE(", true),
    ] {
        for definition in source.split(token).skip(1) {
            let name = definition
                .split(',')
                .next()
                .expect("native option name")
                .trim();
            if columns {
                floats.extend((1..=16).map(|column| format!("{name}{column}")));
            } else {
                floats.push(name.to_owned());
            }
        }
    }
    floats.sort();
    floats.dedup();
    lua.push_str("_ITG_PLAYER_OPTION_FLOATS = {\n");
    for method in floats {
        lua.push_str(&format!("  \"{method}\",\n"));
    }
    lua.push_str("}\n");
    fs::write(out.join("player_option_methods.lua"), lua)
        .expect("could not write generated PlayerOptions Lua method table");
}

fn prepare_pcre(root: &Path, out: &Path) {
    let source = root.join("extern/pcre");
    let generated = out.join("pcre");
    fs::create_dir_all(&generated).unwrap();
    for (from, to) in [
        (source.join("config.h.generic"), generated.join("config.h")),
        (source.join("pcre.h.generic"), generated.join("pcre.h")),
        (
            source.join("pcre_chartables.c.dist"),
            generated.join("pcre_chartables.c"),
        ),
    ] {
        println!("cargo:rerun-if-changed={}", from.display());
        fs::copy(&from, &to)
            .unwrap_or_else(|error| panic!("could not copy {}: {error}", from.display()));
    }
}

fn compile_windows_deps(root: &Path, out: &Path) {
    let pcre = root.join("extern/pcre");
    let mut pcre_build = cc::Build::new();
    pcre_build
        .warnings(false)
        .define("HAVE_CONFIG_H", None)
        .define("PCRE_STATIC", None)
        .include(out.join("pcre"))
        .include(&pcre)
        .file(out.join("pcre/pcre_chartables.c"));
    for source in PCRE_SOURCES {
        pcre_build.file(pcre.join(source));
    }
    pcre_build.compile("pcre");

    cc::Build::new()
        .cpp(true)
        .std("c++17")
        .warnings(false)
        .define("_CRT_SECURE_NO_WARNINGS", None)
        .flag("/EHsc")
        .include(root.join("extern/jsoncpp"))
        .file(root.join("extern/jsoncpp/jsoncpp.cpp"))
        .compile("jsoncpp");
}

fn compile_bundled_lua(root: &Path, target_os: &str) {
    let lua = root.join("extern/lua-5.1/src");
    let mut build = cc::Build::new();
    build.warnings(false).include(&lua);
    if target_os == "windows" {
        // ITGmania's CMakeProject-lua.cmake selects CXX on MSVC. C longjmp
        // across native C++ bindings can corrupt their temporary objects.
        build
            .cpp(true)
            .std("c++17")
            .flag("/TP")
            .flag("/EHsc")
            .define("_CRT_SECURE_NO_WARNINGS", None);
    }
    for source in LUA_SOURCES {
        println!("cargo:rerun-if-changed={}", lua.join(source).display());
        build.file(lua.join(source));
    }
    build.compile("lua5.1");
}

fn git_output(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn require_tree(root: &Path) {
    for relative in [
        "src/Song.h",
        "src/NotesLoaderSM.h",
        "src/NotesLoaderSSC.h",
        "extern/lua-5.1/src/lua.h",
        "extern/jsoncpp/json/json.h",
        "extern/pcre/pcre.h.generic",
        "extern/miniz/miniz.h",
        "CMake/SMDefs.cmake",
    ] {
        let path = root.join(relative);
        assert!(
            path.is_file(),
            "missing ITGmania source {}; initialize the pinned sources with \
             `git submodule update --init` from the harness repository, or set \
             ITGMANIA_ROOT to a complete ITGmania source checkout",
            path.display()
        );
    }
}

fn watch_git_metadata(root: &Path) {
    // A revision can change without changing a compiled source file. Watch Git's
    // actual metadata paths, including submodules whose .git is a pointer file.
    if root.join(".git").is_file() {
        println!("cargo:rerun-if-changed={}", root.join(".git").display());
    }
    let mut paths = vec![
        "HEAD".to_owned(),
        "index".to_owned(),
        "packed-refs".to_owned(),
    ];
    if let Some(reference) = git_output(root, &["symbolic-ref", "-q", "HEAD"]) {
        paths.push(reference);
    }
    for relative in paths {
        if let Some(path) = git_output(root, &["rev-parse", "--git-path", &relative]) {
            let path = root.join(path);
            if path.is_file() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}

fn write_compat_headers(out: &Path, target_os: &str) {
    let config = if target_os == "windows" {
        r#"#pragma once
#define HAVE__MKDIR 1
#define HAVE_SNPRINTF 1
#define HAVE__STRICMP 1
#define HAVE_STRTOF 1
#define PRINTF(a,b)
#define CONST_FUNCTION
#include <direct.h>
#define strcasecmp _stricmp
#define mkdir(path, mode) _mkdir(path)
#ifndef M_PI
#define M_PI 3.1415926535897932384626433832795
#endif
"#
    } else {
        r#"#pragma once
#define HAVE_ALLOCA_H 1
#define HAVE_DIRENT_H 1
#define HAVE_ENDIAN_H 1
#define HAVE_SYS_PARAM_H 1
#define HAVE_SYS_UTSNAME_H 1
#define HAVE_FCNTL_H 1
#define HAVE_UNISTD_H 1
#define HAVE_MKDIR 1
#define HAVE_SNPRINTF 1
#define HAVE_STRCASECMP 1
#define HAVE_STRTOF 1
#define HAVE_M_PI 1
#define HAVE_POSIX_FADVISE 1
#define HAVE_PTHREAD_MUTEX_TIMEDLOCK 1
#define HAVE_PTHREAD_COND_TIMEDWAIT 1
#define PRINTF(a,b) __attribute__((format(__printf__,a,b)))
#define CONST_FUNCTION __attribute__((const))
#include <endian.h>
"#
    };
    fs::write(out.join("config.hpp"), config).unwrap();
    let lua_header = if target_os == "windows" {
        // MSVC Lua and its callers share native C++ linkage and exceptions.
        "#pragma once\n#include \"lua.h\"\n#include \"lauxlib.h\"\n#include \"lualib.h\"\n"
    } else {
        r#"#pragma once
extern "C" {
#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
}
"#
    };
    fs::write(out.join("lua_compat.hpp"), lua_header).unwrap();
}
