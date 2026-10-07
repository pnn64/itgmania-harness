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
    "src/TimingSegments.cpp",
    "src/Steps.cpp",
    "src/StepParityGenerator.cpp",
    "src/StepParityCost.cpp",
    "src/StepParityDatastructs.cpp",
    "src/TechCounts.cpp",
    "src/ScreenMessage.cpp",
    "src/MessageManager.cpp",
    "src/LuaBinding.cpp",
    "src/NotesLoader.cpp",
    "src/NotesLoaderSMA.cpp",
    "src/NotesLoaderSM.cpp",
    "src/NotesLoaderSSC.cpp",
    "src/NotesWriterSM.cpp",
    "src/NotesWriterSSC.cpp",
    "src/ColumnCues.cpp",
    "src/CubicSpline.cpp",
    "src/Actor.cpp",
    "src/ActorFrame.cpp",
    "src/ActorFrameTexture.cpp",
    "src/Tween.cpp",
    "src/RageMath.cpp",
    "src/RageTypes.cpp",
    "src/RageDisplay.cpp",
    "src/RageSurface.cpp",
    "src/Sprite.cpp",
    "src/ModelTypes.cpp",
    "src/Font.cpp",
    "src/FontCharAliases.cpp",
    "src/FontCharmaps.cpp",
    "src/MsdFile.cpp",
    "src/MeasureInfo.cpp",
    "src/RageTexture.cpp",
    "src/RageTextureRenderTarget.cpp",
    "src/RageTextureID.cpp",
    "src/RageFileBasic.cpp",
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
        .unwrap_or_else(|| manifest.join("../itgmania"));
    require_tree(&root);
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
        .include(&out)
        .file(manifest.join("native/oracle_bridge.cpp"))
        .file(manifest.join("native/chart_theme.cpp"))
        .file(manifest.join("native/actor_oracle.cpp"))
        .file(manifest.join("native/options_oracle.cpp"))
        .file(manifest.join("native/runtime_stubs.cpp"));

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
        build.define("_CRT_SECURE_NO_WARNINGS", None);
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
    for relative in ["src/Song.h", "src/NotesLoaderSM.h", "src/NotesLoaderSSC.h"] {
        let path = root.join(relative);
        assert!(path.is_file(), "missing ITGmania source {}", path.display());
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
    fs::write(
        out.join("lua_compat.hpp"),
        r#"#pragma once
extern "C" {
#include "lua.h"
#include "lauxlib.h"
#include "lualib.h"
}
"#,
    )
    .unwrap();
}
