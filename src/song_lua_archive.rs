use crate::{song_lua_baseline, song_lua_oracle};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const ARCHIVE_SCHEMA_VERSION: u32 = 1;
// ActorUtil::InitFileTypeLists classifies these as movie-backed Sprites.
const MOVIE_EXTENSIONS: &[&str] = &[
    "avi", "f4v", "flv", "mkv", "mp4", "mpeg", "mpg", "mov", "ogv", "webm", "wmv",
];

#[derive(Debug)]
pub struct Report {
    pub archives: usize,
    pub source_bytes: u64,
    pub archive_bytes: u64,
    pub index: PathBuf,
}

#[derive(Deserialize)]
struct SemanticManifest {
    fixture_schema_version: u32,
    oracle_schema_version: u32,
    harness_version: String,
    itgmania: SemanticItgmania,
    simfiles: Vec<SemanticEntry>,
}

#[derive(Deserialize)]
struct SemanticItgmania {
    git_revision: String,
    git_dirty: String,
}

#[derive(Deserialize)]
struct SemanticEntry {
    simfile: String,
    fixture: String,
    title: String,
    status: String,
    itgmania: Option<SemanticItgmania>,
}

#[derive(Serialize)]
struct ArchiveManifest {
    archive_schema_version: u32,
    fixture_schema_version: u32,
    oracle_schema_version: u32,
    harness_version: String,
    itgmania: ArchiveItgmania,
    chart: ChartMetadata,
    runtime: RuntimeMetadata,
    lua_closure: LuaClosure,
    textures: Vec<TextureMetadata>,
    required_assets: Vec<AssetReference>,
    files: Vec<FileMetadata>,
}

#[derive(Serialize)]
struct ArchiveItgmania {
    source_revision: String,
    source_dirty: String,
    execution: &'static str,
    launches_executable: bool,
}

#[derive(Serialize)]
struct ChartMetadata {
    title: String,
    source_path: String,
    simfile: String,
    trace: String,
}

#[derive(Serialize)]
struct RuntimeMetadata {
    display: DisplayMetadata,
    update_hz: f64,
    random_state: RandomState,
}

#[derive(Default, Serialize)]
struct DisplayMetadata {
    width: f64,
    height: f64,
    logical_width: f64,
    logical_height: f64,
}

#[derive(Serialize)]
struct RandomState {
    source: &'static str,
    seed: Option<u64>,
    reproducible: bool,
    reseeds: Vec<u64>,
}

#[derive(Serialize)]
struct LuaClosure {
    strategy: &'static str,
    files: Vec<String>,
    external_references: Vec<String>,
}

#[derive(Clone, Serialize)]
struct TextureMetadata {
    reference: String,
    width: Option<u32>,
    height: Option<u32>,
    frames: Option<[u32; 2]>,
    local_path: Option<String>,
    source_sha256: Option<String>,
}

#[derive(Serialize)]
struct AssetReference {
    reference: String,
    kind: &'static str,
    local_path: Option<String>,
    exists: bool,
}

#[derive(Serialize)]
struct FileMetadata {
    path: String,
    role: &'static str,
    bytes: usize,
    sha256: String,
}

#[derive(Serialize)]
struct ArchiveIndex {
    archive_schema_version: u32,
    hash: &'static str,
    archives: Vec<ArchiveIndexEntry>,
}

#[derive(Serialize)]
struct ArchiveIndexEntry {
    title: String,
    source_simfile: String,
    harness_version: String,
    archive: String,
    sha256: String,
    compressed_bytes: u64,
}

pub fn generate(songs_root: &Path, trace_root: &Path, output_dir: &Path) -> Result<Report, Error> {
    let songs_root = canonical_dir("open song corpus", songs_root)?;
    let trace_root = canonical_dir("open semantic traces", trace_root)?;
    fs::create_dir_all(output_dir)
        .map_err(|source| Error::io("create archive directory", output_dir, source))?;
    let semantic_path = trace_root.join("_semantic_manifest.json");
    let semantic: SemanticManifest = read_json(&semantic_path)?;
    let mut index_entries = Vec::with_capacity(semantic.simfiles.len());
    let mut source_bytes = 0u64;
    let mut archive_bytes = 0u64;

    for entry in semantic
        .simfiles
        .iter()
        .filter(|entry| entry.status == "ok")
    {
        let simfile = checked_join(&songs_root, &entry.simfile)?;
        let trace_path = checked_join(&trace_root, &entry.fixture)?;
        let trace_bytes = fs::read(&trace_path)
            .map_err(|source| Error::io("read semantic trace", &trace_path, source))?;
        let trace: Value = serde_json::from_slice(&trace_bytes)
            .map_err(|source| Error::json(&trace_path, source))?;
        let (members, manifest) = archive_members(
            &songs_root,
            &simfile,
            &trace_bytes,
            &trace,
            &semantic,
            entry,
        )?;
        source_bytes += members
            .values()
            .map(|bytes| bytes.len() as u64)
            .sum::<u64>();
        let temp_path = output_dir.join(format!(".song-lua-archive-{}.tmp", std::process::id()));
        write_archive(&temp_path, members, &manifest)?;
        let digest = hash_file(&temp_path)?;
        let filename = format!("{digest}.tar.zst");
        let archive_path = output_dir.join(&filename);
        let compressed_bytes = fs::metadata(&temp_path)
            .map_err(|source| Error::io("stat song archive", &temp_path, source))?
            .len();
        if archive_path.exists() {
            fs::remove_file(&temp_path).map_err(|source| {
                Error::io("remove duplicate temporary archive", &temp_path, source)
            })?;
        } else {
            fs::rename(&temp_path, &archive_path).map_err(|source| {
                Error::io("publish content-addressed archive", &archive_path, source)
            })?;
        }
        archive_bytes += compressed_bytes;
        index_entries.push(ArchiveIndexEntry {
            title: entry.title.clone(),
            source_simfile: entry.simfile.clone(),
            harness_version: semantic.harness_version.clone(),
            archive: filename,
            sha256: digest,
            compressed_bytes,
        });
    }

    index_entries.sort_by(|left, right| left.source_simfile.cmp(&right.source_simfile));
    let index_path = output_dir.join("index.json");
    song_lua_baseline::write_json(
        &index_path,
        &ArchiveIndex {
            archive_schema_version: ARCHIVE_SCHEMA_VERSION,
            hash: "sha256-compressed-archive",
            archives: index_entries,
        },
    )
    .map_err(|error| Error(error.to_string()))?;
    Ok(Report {
        archives: semantic
            .simfiles
            .iter()
            .filter(|entry| entry.status == "ok")
            .count(),
        source_bytes,
        archive_bytes,
        index: index_path,
    })
}

fn archive_members(
    songs_root: &Path,
    simfile: &Path,
    trace_bytes: &[u8],
    trace: &Value,
    semantic: &SemanticManifest,
    entry: &SemanticEntry,
) -> Result<(BTreeMap<String, Vec<u8>>, ArchiveManifest), Error> {
    let song_dir = simfile.parent().unwrap_or_else(|| Path::new("."));
    let simfile_name = simfile
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            Error(format!(
                "simfile path is not portable UTF-8: {}",
                simfile.display()
            ))
        })?;
    let simfile_member = format!("song/{simfile_name}");
    let trace_member = "trace/semantic.json".to_owned();
    let simfile_bytes =
        fs::read(simfile).map_err(|source| Error::io("read archive simfile", simfile, source))?;

    let (lua_paths, external_lua) = lua_dependency_closure(simfile, song_dir, trace)?;
    let mut members = BTreeMap::new();
    members.insert(simfile_member.clone(), simfile_bytes);
    members.insert(trace_member.clone(), trace_bytes.to_vec());
    let mut lua_members = Vec::with_capacity(lua_paths.len());
    for path in &lua_paths {
        let relative = portable_relative(song_dir, path)?;
        let member = format!("song/{relative}");
        let bytes =
            fs::read(path).map_err(|source| Error::io("read Lua dependency", path, source))?;
        members.insert(member.clone(), bytes);
        lua_members.push(member);
    }
    lua_members.sort();

    let mut texture_map = texture_references(trace, song_dir)?;
    let asset_refs = asset_references(trace, song_dir, &lua_paths)?;
    for asset in asset_refs.iter().filter(|asset| asset.kind == "texture") {
        if !texture_map.contains_key(&asset.reference) {
            texture_map.insert(
                asset.reference.clone(),
                texture_metadata(&asset.reference, None, song_dir)?,
            );
        }
    }
    let mut asset_members = BTreeSet::new();
    for asset in &asset_refs {
        let Some(relative) = &asset.local_path else {
            continue;
        };
        let source = checked_join(song_dir, relative)?;
        let member = format!("song/{relative}");
        let bytes = fs::read(&source)
            .map_err(|error| Error::io("read required song asset", &source, error))?;
        members.insert(member.clone(), bytes);
        asset_members.insert(member);
    }
    // Runtime reads/listings may depend on files without a texture or Lua extension.
    let mut runtime_files = BTreeSet::new();
    for read in trace
        .get("file_reads")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if read.get("exists").and_then(Value::as_bool) == Some(true)
            && read.get("generated").and_then(Value::as_bool) != Some(true)
        {
            if let Some(path) = read
                .get("path")
                .and_then(Value::as_str)
                .and_then(|v| v.strip_prefix("song:/"))
            {
                runtime_files.insert(path.to_owned());
            }
        }
    }
    for query in trace
        .get("directory_queries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for path in query
            .get("files")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter_map(|v| v.strip_prefix("song:/"))
        {
            if checked_join(song_dir, path)?.is_file() {
                runtime_files.insert(path.to_owned());
            }
        }
    }
    for relative in runtime_files {
        let path = checked_join(song_dir, &relative)?;
        let member = format!("song/{relative}");
        members.insert(
            member.clone(),
            fs::read(&path).map_err(|error| Error::io("read runtime dependency", &path, error))?,
        );
        asset_members.insert(member);
    }
    let files = members
        .iter()
        .map(|(path, bytes)| FileMetadata {
            path: path.clone(),
            role: if path == &simfile_member {
                "simfile"
            } else if path == &trace_member {
                "semantic-render-trace"
            } else if asset_members.contains(path) {
                "required-asset"
            } else {
                "lua-dependency"
            },
            bytes: bytes.len(),
            sha256: hash_bytes(bytes),
        })
        .collect();
    let itgmania = entry.itgmania.as_ref().unwrap_or(&semantic.itgmania);
    let manifest = ArchiveManifest {
        archive_schema_version: ARCHIVE_SCHEMA_VERSION,
        fixture_schema_version: semantic.fixture_schema_version,
        oracle_schema_version: semantic.oracle_schema_version,
        harness_version: semantic.harness_version.clone(),
        itgmania: ArchiveItgmania {
            source_revision: itgmania.git_revision.clone(),
            source_dirty: itgmania.git_dirty.clone(),
            execution: "embedded_bundled_lua",
            launches_executable: false,
        },
        chart: ChartMetadata {
            title: entry.title.clone(),
            source_path: song_lua_baseline::relative_path(songs_root, simfile)
                .map_err(|error| Error(error.to_string()))?,
            simfile: simfile_member,
            trace: trace_member,
        },
        runtime: RuntimeMetadata {
            display: display_metadata(trace),
            update_hz: trace
                .get("update_fps")
                .and_then(Value::as_f64)
                .unwrap_or(60.0),
            random_state: RandomState {
                source: "ITGmania MersenneTwister",
                seed: trace.get("random_seed").and_then(Value::as_u64),
                reproducible: trace.get("random_seed").and_then(Value::as_u64).is_some()
                    && trace
                        .get("random_reseeds")
                        .and_then(Value::as_array)
                        .is_none_or(|seeds| {
                            seeds.iter().all(|seed| {
                                seed.as_u64().is_some_and(|v| v > 0 && v <= i32::MAX as u64)
                            })
                        }),
                reseeds: trace
                    .get("random_reseeds")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_u64)
                    .collect(),
            },
        },
        lua_closure: LuaClosure {
            strategy: "executed-sources-plus-static-loads",
            files: lua_members,
            external_references: external_lua,
        },
        textures: texture_map.into_values().collect(),
        required_assets: asset_refs,
        files,
    };
    Ok((members, manifest))
}

fn lua_dependency_closure(
    simfile: &Path,
    song_dir: &Path,
    trace: &Value,
) -> Result<(BTreeSet<PathBuf>, Vec<String>), Error> {
    let mut references = BTreeSet::new();
    collect_string_values(trace, &mut |value| {
        if is_lua_reference(value) {
            references.insert(value.to_owned());
        }
    });
    let discovery = song_lua_oracle::load(simfile).map_err(|error| {
        Error(format!(
            "could not discover song Lua roots for {}: {error}",
            simfile.display()
        ))
    })?;
    for entry in discovery
        .changes
        .into_iter()
        .filter_map(|change| change.lua_entry)
    {
        references.insert(format!("file:{entry}"));
    }

    let mut files = BTreeSet::new();
    let mut external = BTreeSet::new();
    let mut pending = VecDeque::from_iter(references);
    while let Some(reference) = pending.pop_front() {
        let resolved = resolve_lua_reference(song_dir, None, &reference);
        let Some(path) = resolved else {
            external.insert(reference);
            continue;
        };
        let canonical = match path.canonicalize() {
            Ok(path) if path.starts_with(song_dir) => path,
            _ => {
                external.insert(reference);
                continue;
            }
        };
        if !files.insert(canonical.clone()) {
            continue;
        }
        let source = fs::read_to_string(&canonical)
            .map_err(|source| Error::io("read Lua dependency source", &canonical, source))?;
        for static_reference in quoted_lua_references(&source) {
            if let Some(path) =
                resolve_lua_reference(song_dir, canonical.parent(), &static_reference)
            {
                pending.push_back(format!("file:{}", path.display()));
            } else {
                external.insert(static_reference);
            }
        }
    }
    Ok((files, external.into_iter().collect()))
}

fn resolve_lua_reference(
    song_dir: &Path,
    source_dir: Option<&Path>,
    reference: &str,
) -> Option<PathBuf> {
    let path = if let Some(path) = reference.strip_prefix("file:") {
        PathBuf::from(path)
    } else if let Some(path) = reference.strip_prefix("song:/") {
        song_dir.join(normalized_relative(path)?)
    } else if reference.contains(":/") {
        return None;
    } else if let Some(source_dir) = source_dir {
        let local = source_dir.join(reference);
        if local.is_file() {
            local
        } else {
            song_dir.join(normalized_relative(reference)?)
        }
    } else {
        song_dir.join(normalized_relative(reference)?)
    };
    path.is_file().then_some(path)
}

fn quoted_lua_references(source: &str) -> Vec<String> {
    quoted_values(source)
        .into_iter()
        .filter(|value| is_lua_reference(value))
        .collect()
}

fn quoted_values(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index..].starts_with(b"--") {
            index += 2;
            let mut opening = index + 1;
            while bytes.get(opening) == Some(&b'=') {
                opening += 1;
            }
            if bytes.get(index) == Some(&b'[') && bytes.get(opening) == Some(&b'[') {
                let closing = format!("]{}]", "=".repeat(opening - index - 1));
                index = source[opening + 1..]
                    .find(&closing)
                    .map_or(bytes.len(), |end| opening + 1 + end + closing.len());
            } else {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
            }
            continue;
        }
        let quote = bytes[index];
        if quote != b'\'' && quote != b'"' {
            index += 1;
            continue;
        }
        let start = index + 1;
        index = start;
        while index < bytes.len() && bytes[index] != quote {
            index += 1 + usize::from(bytes[index] == b'\\' && index + 1 < bytes.len());
        }
        if let Some(value) = source.get(start..index) {
            out.push(value.to_owned());
        }
        index += 1;
    }
    out
}

fn texture_references(
    trace: &Value,
    song_dir: &Path,
) -> Result<BTreeMap<String, TextureMetadata>, Error> {
    let mut textures = BTreeMap::new();
    if let Some(tracks) = trace
        .get("projected_vertex_tracks")
        .and_then(Value::as_array)
    {
        for track in tracks {
            let Some(reference) = track.get("texture").and_then(Value::as_str) else {
                continue;
            };
            if reference.is_empty() {
                continue;
            }
            let dimensions = track.get("texture_size").and_then(array_dimensions);
            textures.insert(
                reference.to_owned(),
                texture_metadata(reference, dimensions, song_dir)?,
            );
        }
    }
    collect_string_values(trace, &mut |value| {
        if is_texture_reference(value) {
            textures
                .entry(value.to_owned())
                .or_insert_with(|| TextureMetadata {
                    reference: value.to_owned(),
                    width: None,
                    height: None,
                    frames: frame_grid(value),
                    local_path: None,
                    source_sha256: None,
                });
        }
    });
    for texture in textures.values_mut() {
        if texture.local_path.is_none() {
            *texture = texture_metadata(&texture.reference, None, song_dir)?;
        }
    }
    Ok(textures)
}

fn texture_metadata(
    reference: &str,
    dimensions: Option<[u32; 2]>,
    song_dir: &Path,
) -> Result<TextureMetadata, Error> {
    let path = resolve_song_asset(song_dir, reference);
    let dimensions = dimensions.or_else(|| {
        path.as_deref()
            .and_then(|path| crate::actor_conformance::texture_source_size(path).ok())
    });
    let local_path = path
        .as_deref()
        .map(|path| portable_relative(song_dir, path))
        .transpose()?;
    let source_sha256 = path.as_deref().map(hash_file).transpose()?;
    Ok(TextureMetadata {
        reference: reference.to_owned(),
        width: dimensions.map(|value| value[0]),
        height: dimensions.map(|value| value[1]),
        frames: frame_grid(reference),
        local_path,
        source_sha256,
    })
}

fn asset_references(
    trace: &Value,
    song_dir: &Path,
    lua_paths: &BTreeSet<PathBuf>,
) -> Result<Vec<AssetReference>, Error> {
    let mut references = BTreeMap::<String, Option<PathBuf>>::new();
    collect_string_values(trace, &mut |value| {
        if is_asset_reference(value) {
            insert_asset_reference(&mut references, song_dir, None, value);
        }
    });
    if let Some(definitions) = trace.get("actor_definitions").and_then(Value::as_array) {
        for definition in definitions {
            let source_dir = definition
                .get("source")
                .and_then(Value::as_str)
                .and_then(|source| resolve_lua_reference(song_dir, None, source))
                .and_then(|source| source.parent().map(Path::to_owned));
            if let Some(properties) = definition.get("properties").and_then(Value::as_object) {
                for value in properties.values().filter_map(Value::as_str) {
                    insert_asset_reference(&mut references, song_dir, source_dir.as_deref(), value);
                }
            }
        }
    }
    for lua_path in lua_paths {
        let source = fs::read_to_string(lua_path)
            .map_err(|error| Error::io("scan Lua asset references", lua_path, error))?;
        for value in quoted_values(&source) {
            insert_asset_reference(&mut references, song_dir, lua_path.parent(), &value);
        }
    }
    // Model::LoadMaterialsFromMilkshapeAscii resolves diffuse and alpha
    // textures relative to the material file, rather than the Lua source.
    let models: Vec<_> = references
        .values()
        .flatten()
        .filter(|path| matches_extension(&path.to_string_lossy(), &["txt"]))
        .cloned()
        .collect();
    for model in models {
        let bytes = fs::read(&model)
            .map_err(|error| Error::io("scan model material references", &model, error))?;
        let source = String::from_utf8_lossy(&bytes);
        if !source.lines().any(|line| line.starts_with("Materials:")) {
            continue;
        }
        for line in source.lines() {
            if let Some(value) = line
                .trim()
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
            {
                insert_asset_reference(
                    &mut references,
                    song_dir,
                    model.parent(),
                    &value.replace('\\', "/"),
                );
            }
        }
    }
    // AnimatedTexture::Load reads FrameNNNN paths from the INI's directory.
    let animations: Vec<_> = references
        .values()
        .flatten()
        .filter(|path| matches_extension(&path.to_string_lossy(), &["ini"]))
        .cloned()
        .collect();
    for animation in animations {
        let bytes = fs::read(&animation)
            .map_err(|error| Error::io("scan animated texture frames", &animation, error))?;
        let source = String::from_utf8_lossy(&bytes);
        let mut animated = false;
        for line in source.lines().map(str::trim) {
            if line.starts_with('[') {
                animated = line.eq_ignore_ascii_case("[AnimatedTexture]");
            } else if animated && let Some((key, value)) = line.split_once('=') {
                if key.trim().strip_prefix("Frame").is_some_and(|suffix| {
                    suffix.len() == 4 && suffix.bytes().all(|byte| byte.is_ascii_digit())
                }) {
                    insert_asset_reference(
                        &mut references,
                        song_dir,
                        animation.parent(),
                        &value.trim().replace('\\', "/"),
                    );
                }
            }
        }
    }
    // Font::GetFontPaths loads every page sharing the INI's basename.
    // Those images need not appear as literal texture paths in the Lua.
    let fonts: Vec<_> = references
        .values()
        .flatten()
        .filter(|path| matches_extension(&path.to_string_lossy(), &["ini"]))
        .cloned()
        .collect();
    for font in fonts {
        let Some(parent) = font.parent() else {
            continue;
        };
        let Some(stem) = font.file_stem().and_then(|name| name.to_str()) else {
            continue;
        };
        for page in
            fs::read_dir(parent).map_err(|error| Error::io("scan font pages", parent, error))?
        {
            let page = page
                .map_err(|error| Error::io("read font page", parent, error))?
                .path();
            let name = page.file_name().unwrap_or_default().to_string_lossy();
            if name
                .get(..stem.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(stem))
                && matches_extension(&name, &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
            {
                insert_asset_reference(&mut references, song_dir, Some(parent), &name);
            }
        }
    }
    references
        .into_iter()
        .map(|(reference, path)| {
            let local_path = path
                .as_deref()
                .map(|path| portable_relative(song_dir, path))
                .transpose()?;
            Ok(AssetReference {
                kind: asset_kind(&reference),
                exists: path.is_some(),
                reference,
                local_path,
            })
        })
        .collect()
}

fn insert_asset_reference(
    references: &mut BTreeMap<String, Option<PathBuf>>,
    song_dir: &Path,
    source_dir: Option<&Path>,
    value: &str,
) {
    if let Some(path) = resolve_asset_relative(song_dir, source_dir, value) {
        if let Ok(relative) = portable_relative(song_dir, &path) {
            references.insert(format!("song:/{relative}"), Some(path));
        }
    } else if is_asset_reference(value) {
        references.entry(value.to_owned()).or_insert(None);
    }
}

fn resolve_song_asset(song_dir: &Path, reference: &str) -> Option<PathBuf> {
    resolve_asset_relative(song_dir, None, reference)
}

fn resolve_asset_relative(
    song_dir: &Path,
    source_dir: Option<&Path>,
    reference: &str,
) -> Option<PathBuf> {
    let path = if let Some(relative) = reference.strip_prefix("song:/") {
        song_dir.join(normalized_relative(relative)?)
    } else if reference.contains(":/") || Path::new(reference).is_absolute() {
        return None;
    } else {
        source_dir.unwrap_or(song_dir).join(reference)
    };
    if path.is_file() {
        let path = path
            .canonicalize()
            .ok()
            .filter(|path| path.starts_with(song_dir))?;
        return is_asset_path(&path).then_some(path);
    }
    let parent = path.parent()?;
    let name = path.file_name()?.to_string_lossy();
    let has_extension = path.extension().is_some();
    const EXTENSIONS: &[&str] = &[
        "png", "jpg", "jpeg", "gif", "webp", "bmp", "ogg", "wav", "mp3", "lua", "xml", "frag",
        "vert", "ini", "txt",
    ];
    // ActorUtil::ResolvePath appends '*' after an exact miss. Frame hints
    // such as "overlay 3x4.png" are therefore part of the resolved asset.
    let mut matches = fs::read_dir(parent).ok()?.filter_map(|entry| {
        let candidate = entry.ok()?.path();
        let candidate_name = candidate.file_name()?.to_string_lossy();
        let extension = candidate.extension()?.to_string_lossy();
        if ((has_extension && candidate_name.eq_ignore_ascii_case(&name))
            || (!has_extension
                && candidate_name
                    .to_ascii_lowercase()
                    .starts_with(&name.to_ascii_lowercase())))
            && EXTENSIONS
                .iter()
                .chain(MOVIE_EXTENSIONS)
                .any(|known| extension.eq_ignore_ascii_case(known))
        {
            Some(candidate)
        } else {
            None
        }
    });
    let candidate = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    candidate
        .canonicalize()
        .ok()
        .filter(|path| path.starts_with(song_dir))
}

fn is_asset_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png"
                    | "jpg"
                    | "jpeg"
                    | "gif"
                    | "webp"
                    | "bmp"
                    | "ogg"
                    | "wav"
                    | "mp3"
                    | "frag"
                    | "vert"
                    | "ini"
                    | "txt"
            )
        })
        || matches_extension(&path.to_string_lossy(), MOVIE_EXTENSIONS)
}

fn collect_string_values(value: &Value, visit: &mut impl FnMut(&str)) {
    match value {
        Value::String(value) => visit(value),
        Value::Array(values) => values
            .iter()
            .for_each(|value| collect_string_values(value, visit)),
        Value::Object(values) => values
            .values()
            .for_each(|value| collect_string_values(value, visit)),
        _ => {}
    }
}

fn display_metadata(trace: &Value) -> DisplayMetadata {
    let display = trace.get("display").unwrap_or(&Value::Null);
    DisplayMetadata {
        width: display.get("width").and_then(Value::as_f64).unwrap_or(0.0),
        height: display.get("height").and_then(Value::as_f64).unwrap_or(0.0),
        logical_width: display
            .get("logical_width")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
        logical_height: display
            .get("logical_height")
            .and_then(Value::as_f64)
            .unwrap_or(0.0),
    }
}

fn write_archive(
    path: &Path,
    members: BTreeMap<String, Vec<u8>>,
    manifest: &ArchiveManifest,
) -> Result<(), Error> {
    let output = File::create(path)
        .map_err(|source| Error::io("create compressed archive", path, source))?;
    let mut encoder = zstd::stream::write::Encoder::new(output, 12)
        .map_err(|source| Error::io("start zstd archive", path, source))?;
    encoder
        .include_checksum(true)
        .map_err(|source| Error::io("configure zstd archive", path, source))?;
    {
        let mut archive = tar::Builder::new(&mut encoder);
        let manifest_bytes = serde_json::to_vec_pretty(manifest)
            .map_err(|source| Error(format!("could not serialize archive manifest: {source}")))?;
        append_member(&mut archive, "manifest.json", &manifest_bytes)?;
        for (member, bytes) in members {
            append_member(&mut archive, &member, &bytes)?;
        }
        archive
            .finish()
            .map_err(|source| Error::io("finish tar archive", path, source))?;
    }
    encoder
        .finish()
        .map_err(|source| Error::io("finish zstd archive", path, source))?;
    Ok(())
}

fn append_member(
    archive: &mut tar::Builder<&mut zstd::stream::write::Encoder<'_, File>>,
    path: &str,
    bytes: &[u8],
) -> Result<(), Error> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    header.set_mode(0o644);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    archive
        .append_data(&mut header, path, bytes)
        .map_err(|source| Error(format!("could not append archive member {path}: {source}")))
}

fn frame_grid(reference: &str) -> Option<[u32; 2]> {
    let stem = Path::new(reference).file_stem()?.to_string_lossy();
    stem.split_whitespace().rev().find_map(|part| {
        let (columns, rows) = part.split_once('x')?;
        Some([columns.parse().ok()?, rows.parse().ok()?])
    })
}

fn array_dimensions(value: &Value) -> Option<[u32; 2]> {
    let values = value.as_array()?;
    Some([
        values.first()?.as_f64()?.round() as u32,
        values.get(1)?.as_f64()?.round() as u32,
    ])
}

fn is_lua_reference(value: &str) -> bool {
    value
        .split(':')
        .next()
        .is_some_and(|path| path.to_ascii_lowercase().ends_with(".lua"))
        || value.to_ascii_lowercase().ends_with(".lua")
}

fn is_texture_reference(value: &str) -> bool {
    value.starts_with("song:/") && asset_kind(value) == "texture"
}

fn is_asset_reference(value: &str) -> bool {
    value.starts_with("song:/") && is_asset_path(Path::new(value))
}

fn matches_extension(value: &str, extensions: &[&str]) -> bool {
    Path::new(value)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extensions
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

fn asset_kind(reference: &str) -> &'static str {
    if matches_extension(reference, &["png", "jpg", "jpeg", "gif", "webp", "bmp"])
        || matches_extension(reference, MOVIE_EXTENSIONS)
    {
        "texture"
    } else if matches_extension(reference, &["ogg", "wav", "mp3"]) {
        "audio"
    } else if matches_extension(reference, &["frag", "vert"]) {
        "shader"
    } else {
        "other"
    }
}

fn normalized_relative(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    let normalized = path.replace('\\', "/");
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(value) => out.push(value),
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

fn checked_join(root: &Path, relative: &str) -> Result<PathBuf, Error> {
    let path = root.join(
        normalized_relative(relative)
            .ok_or_else(|| Error(format!("unsafe relative path `{relative}`")))?,
    );
    let canonical = path
        .canonicalize()
        .map_err(|source| Error::io("open archive input", &path, source))?;
    canonical
        .starts_with(root)
        .then_some(canonical)
        .ok_or_else(|| Error(format!("archive input escapes root: {}", path.display())))
}

fn portable_relative(root: &Path, path: &Path) -> Result<String, Error> {
    let canonical = path
        .canonicalize()
        .map_err(|source| Error::io("resolve archive member", path, source))?;
    canonical
        .strip_prefix(root)
        .map(song_lua_baseline::slash_path)
        .map_err(|_| {
            Error(format!(
                "archive member {} escapes song directory {}",
                path.display(),
                root.display()
            ))
        })
}

fn canonical_dir(action: &'static str, path: &Path) -> Result<PathBuf, Error> {
    path.canonicalize()
        .map_err(|source| Error::io(action, path, source))
        .and_then(|path| {
            path.is_dir()
                .then_some(path.clone())
                .ok_or_else(|| Error(format!("{} is not a directory", path.display())))
        })
}

fn hash_bytes(bytes: &[u8]) -> String {
    encode_hash(Sha256::digest(bytes))
}

fn hash_file(path: &Path) -> Result<String, Error> {
    let mut file =
        File::open(path).map_err(|source| Error::io("open file for hashing", path, source))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|source| Error::io("hash file", path, source))?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(encode_hash(hasher.finalize()))
}

fn encode_hash(hash: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;

    let bytes = hash.as_ref();
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(encoded, "{byte:02x}").expect("writing to a String cannot fail");
    }
    encoded
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Error> {
    let bytes = fs::read(path).map_err(|source| Error::io("read JSON", path, source))?;
    serde_json::from_slice(&bytes).map_err(|source| Error::json(path, source))
}

#[derive(Debug)]
pub struct Error(String);

impl Error {
    fn io(action: &str, path: &Path, source: std::io::Error) -> Self {
        Self(format!("could not {action} {}: {source}", path.display()))
    }

    fn json(path: &Path, source: serde_json::Error) -> Self {
        Self(format!("invalid JSON in {}: {source}", path.display()))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(itgmania_oracle)]
    #[test]
    fn archive_runtime_sprite() {
        use crate::song_lua_headless::{Context, Entry, evaluate_with_noteskin};
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("Sprite fixtures");
        let simfile = root.join("sprite-background-assets.sm");
        let context = Context {
            simfile: &simfile,
            song_dir: &root,
            title: "Runtime sprite asset",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 4.0,
            bpm: 120.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate_with_noteskin(
            &[Entry {
                path: root.join("sprite-background-assets.lua"),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
            None,
        )
        .expect("native parsed background and fallback helper");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        assert!(
            trace["projected_vertex_tracks"]
                .as_array()
                .expect("tracks")
                .is_empty()
        );
        assert_eq!(trace["texture_requests"][0]["path"], "song:/fit-rect.png");
        let source = serde_json::to_vec(&trace).expect("native trace bytes");
        let provenance = || SemanticItgmania {
            git_revision: "native-test".into(),
            git_dirty: "false".into(),
        };
        let semantic = SemanticManifest {
            fixture_schema_version: 1,
            oracle_schema_version: 2,
            harness_version: env!("CARGO_PKG_VERSION").into(),
            itgmania: provenance(),
            simfiles: vec![],
        };
        let entry = SemanticEntry {
            simfile: "sprite-background-assets.sm".into(),
            fixture: "semantic.json".into(),
            title: context.title.into(),
            status: "ok".into(),
            itgmania: Some(provenance()),
        };
        let (members, manifest) =
            archive_members(&root, &simfile, &source, &trace, &semantic, &entry)
                .expect("complete runtime dependency archive");
        let image = fs::read(root.join("fit-rect.png")).expect("native loaded image");
        assert_eq!(members.get("song/fit-rect.png"), Some(&image));
        assert!(
            manifest
                .required_assets
                .iter()
                .any(|asset| asset.local_path.as_deref() == Some("fit-rect.png") && asset.exists)
        );
        let texture = manifest
            .textures
            .iter()
            .find(|texture| texture.reference == "song:/fit-rect.png")
            .expect("runtime image metadata");
        assert_eq!((texture.width, texture.height), (Some(64), Some(32)));
        assert_eq!(
            texture.source_sha256.as_deref(),
            Some(hash_bytes(&image).as_str())
        );
    }

    #[test]
    fn indexed_archive_metadata_uses_native_frame_dimensions() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/actors/bitmap-loader")
            .canonicalize()
            .expect("native indexed asset root");
        for name in ["frame-offset.gif", "os2-4.bmp"] {
            let texture = texture_metadata(&format!("song:/{name}"), None, &root)
                .expect("indexed texture metadata");
            assert_eq!((texture.width, texture.height), (Some(8), Some(8)));
            assert_eq!(
                texture.source_sha256,
                Some(hash_file(&root.join(name)).unwrap())
            );
        }
    }

    #[test]
    fn model_archives_include_materials_and_frames() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tests/model-archive");
        let nested = root.join("lua/body");
        fs::create_dir_all(nested.join("textures")).expect("model directory");
        fs::write(
            root.join("model.sm"),
            "#TITLE:Model;#FGCHANGES:0=lua/body=1;",
        )
        .expect("simfile");
        fs::write(
            nested.join("default.lua"),
            "return Def.Model{Meshes='wall.txt', Materials='wall.txt', Bones='wall.txt'}",
        )
        .expect("model actor");
        let model = b"// MilkShape 3D ASCII\nMeshes: 0\nMaterials: 1\n\"material\"\n0 0 0 1\n1 1 1 1\n0 0 0 1\n0 0 0 1\n0\n1\n\"surface.ini\"\n\"textures\\mask.png\"\nBones: 0\n";
        fs::write(nested.join("wall.txt"), model).expect("model materials");
        fs::write(nested.join("surface.ini"),
            "[AnimatedTexture]\nFrame0000=textures/first.png\nDelay0000=0.1\nFrame0001=textures/second.png\nDelay0001=0.2\n")
            .expect("animated diffuse texture");
        for name in ["first.png", "second.png", "mask.png", "unrelated.png"] {
            fs::write(nested.join("textures").join(name), name).expect("texture bytes");
        }
        let root = root.canonicalize().expect("fixture root");
        let trace = serde_json::json!({"actor_definitions": [{
            "source": "song:/lua/body/default.lua",
            "properties": {"Meshes": "wall.txt", "Materials": "wall.txt", "Bones": "wall.txt"}
        }]});
        let provenance = || SemanticItgmania {
            git_revision: "archive-test".into(),
            git_dirty: "false".into(),
        };
        let semantic = SemanticManifest {
            fixture_schema_version: 1,
            oracle_schema_version: 2,
            harness_version: env!("CARGO_PKG_VERSION").into(),
            itgmania: provenance(),
            simfiles: vec![],
        };
        let entry = SemanticEntry {
            simfile: "model.sm".into(),
            fixture: "semantic.json".into(),
            title: "Model".into(),
            status: "ok".into(),
            itgmania: Some(provenance()),
        };
        let (members, manifest) = archive_members(
            &root,
            &root.join("model.sm"),
            &serde_json::to_vec(&trace).expect("trace bytes"),
            &trace,
            &semantic,
            &entry,
        )
        .expect("model archive");
        assert_eq!(members["song/lua/body/wall.txt"], model);
        for name in ["first.png", "second.png", "mask.png"] {
            let path = format!("song/lua/body/textures/{name}");
            assert_eq!(members[&path], name.as_bytes());
            assert!(
                manifest
                    .textures
                    .iter()
                    .any(|texture| texture.reference == format!("song:/lua/body/textures/{name}"))
            );
        }
        assert!(!members.contains_key("song/lua/body/textures/unrelated.png"));
        assert!(manifest.required_assets.iter().all(|asset| asset.exists));
    }

    #[test]
    fn movie_actor_assets_are_required_textures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tests/movie-actor-assets");
        fs::create_dir_all(&root).expect("fixture directory");
        for extension in MOVIE_EXTENSIONS {
            fs::write(root.join(format!("movie-{extension}.{extension}")), [])
                .expect("movie placeholder");
        }
        let root = root.canonicalize().expect("fixture root");
        for extension in MOVIE_EXTENSIONS {
            let name = format!("movie-{extension}.{extension}");
            let reference = format!("song:/{name}");
            assert!(is_asset_reference(&reference));
            assert!(is_texture_reference(&reference));
            assert_eq!(asset_kind(&reference), "texture");
            assert_eq!(
                resolve_song_asset(&root, &reference),
                Some(root.join(&name))
            );
            assert_eq!(
                resolve_song_asset(&root, &format!("song:/movie-{extension}")),
                Some(root.join(&name)),
            );
            let refs = asset_references(&Value::String(reference), &root, &BTreeSet::new())
                .expect("movie dependency");
            assert_eq!(refs.len(), 1);
            assert!(refs[0].exists);
            assert_eq!(refs[0].local_path.as_deref(), Some(name.as_str()));
            assert_eq!(refs[0].kind, "texture");
        }
    }

    #[test]
    fn hinted_actor_assets_are_archived_without_ambiguous_prefixes() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tests/hinted-actor-assets");
        let nested = root.join("lua");
        fs::create_dir_all(&nested).expect("fixture directory");
        let source = nested.join("default.lua");
        fs::write(&source, "return LoadActor('overlay2')").expect("actor source");
        fs::write(nested.join("overlay2 3x4.png"), []).expect("hinted image");
        fs::write(nested.join("ambiguous 2x2.png"), []).expect("first image");
        fs::write(nested.join("ambiguous 3x4.png"), []).expect("second image");
        let root = root.canonicalize().expect("fixture root");
        let nested = nested.canonicalize().expect("nested source");
        let refs = asset_references(
            &Value::Null,
            &root,
            &BTreeSet::from([source.canonicalize().expect("Lua path")]),
        )
        .expect("asset references");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].local_path.as_deref(), Some("lua/overlay2 3x4.png"));
        assert!(refs[0].exists);
        assert!(resolve_asset_relative(&root, Some(&nested), "ambiguous").is_none());
        assert_eq!(
            resolve_asset_relative(&root, Some(&nested), "overlay2 3x4.png"),
            Some(nested.join("overlay2 3x4.png")),
        );
    }

    #[test]
    fn numbered_font_assets_include_ini_and_implicit_pages() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/tests/numbered-font-assets");
        let nested = root.join("multitap");
        fs::create_dir_all(&nested).expect("font fixture directory");
        let source = nested.join("default.lua");
        fs::write(&source, "return Def.BitmapText{Font='_count 42px.ini'}").expect("font actor");
        fs::write(nested.join("_count 42px.ini"), "[common]\nLine 0=0123\n").expect("font INI");
        for page in [
            "_count 42px [numbers] 4x4.png",
            "_count 42px-stroke.png",
            "unrelated.png",
        ] {
            fs::write(nested.join(page), []).expect("page placeholder");
        }
        let root = root.canonicalize().expect("fixture root");
        let refs = asset_references(
            &Value::Null,
            &root,
            &BTreeSet::from([source.canonicalize().expect("Lua path")]),
        )
        .expect("font dependencies");
        let paths: Vec<_> = refs
            .iter()
            .filter_map(|asset| asset.local_path.as_deref())
            .collect();
        assert_eq!(
            paths,
            [
                "multitap/_count 42px [numbers] 4x4.png",
                "multitap/_count 42px-stroke.png",
                "multitap/_count 42px.ini"
            ]
        );
        assert!(refs.iter().all(|asset| asset.exists));
    }

    #[test]
    fn static_lua_dependencies_are_collected_from_quoted_paths() {
        assert_eq!(
            quoted_lua_references(
                "-- the modder's files\nloadfile('helpers.lua')()\n\
                 --[=[ don't load 'missing.lua' ]=]\n\
                 loadfile(xero.dir..'template/std.lua')()\nTexture='image.png'"
            ),
            ["helpers.lua", "template/std.lua"]
        );
    }

    #[test]
    fn song_relative_lua_loads_resolve_from_nested_sources() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(
            resolve_lua_reference(root, Some(&root.join("src")), "semantic_host.lua"),
            Some(root.join("semantic_host.lua"))
        );
    }

    #[test]
    fn archive_paths_reject_parent_escape() {
        assert!(normalized_relative("../../outside.lua").is_none());
        assert_eq!(
            normalized_relative("lua/../bg/main.lua"),
            Some(PathBuf::from("bg/main.lua"))
        );
    }
}
