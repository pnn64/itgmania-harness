use serde_json::Value;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"ITGSEM\0\0";
const WIRE_VERSION: u32 = 2;
const HOST: &str = include_str!("../semantic_host.lua");
#[cfg(itgmania_oracle)]
const THEME_HELPERS: &[(&str, &str)] = &[
    (
        "01 IniFile.lua",
        include_str!(concat!(
            env!("ITGMANIA_BUILD_ROOT"),
            "/Themes/_fallback/Scripts/01 IniFile.lua"
        )),
    ),
    (
        "02 ThemePrefs.lua",
        include_str!(concat!(
            env!("ITGMANIA_BUILD_ROOT"),
            "/Themes/_fallback/Scripts/02 ThemePrefs.lua"
        )),
    ),
    (
        "02 Utilities.lua",
        include_str!(concat!(
            env!("ITGMANIA_BUILD_ROOT"),
            "/Themes/_fallback/Scripts/02 Utilities.lua"
        )),
    ),
    (
        "02 Sprite.lua",
        include_str!(concat!(
            env!("ITGMANIA_BUILD_ROOT"),
            "/Themes/_fallback/Scripts/02 Sprite.lua"
        )),
    ),
    (
        "02 Colors.lua",
        include_str!(concat!(
            env!("ITGMANIA_BUILD_ROOT"),
            "/Themes/_fallback/Scripts/02 Colors.lua"
        )),
    ),
];
#[cfg(not(itgmania_oracle))]
const THEME_HELPERS: &[(&str, &str)] = &[];
#[cfg(itgmania_oracle)]
const ACTOR_HELPERS: &str = include_str!(concat!(
    env!("ITGMANIA_BUILD_ROOT"), "/Themes/_fallback/Scripts/02 Actor.lua"
));
#[cfg(not(itgmania_oracle))]
const ACTOR_HELPERS: &str = "";
const PLAYER_OPTION_METHODS: &str =
    include_str!(concat!(env!("OUT_DIR"), "/player_option_methods.lua"));

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub layer: &'static str,
    pub index: u32,
    pub start_beat: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct BpmSegment {
    pub beat: f32,
    pub bpm: f32,
}

#[derive(Debug)]
pub struct Context<'a> {
    pub simfile: &'a Path,
    pub song_dir: &'a Path,
    pub title: &'a str,
    pub difficulty: &'a str,
    pub steps_type: &'a str,
    pub description: &'a str,
    pub max_beat: f32,
    pub bpm: f32,
    pub bpm_segments: &'a [BpmSegment],
    pub beat_step: f32,
    pub max_events: u32,
    pub random_seed: u32,
}

pub fn evaluate(entries: &[Entry], context: &Context<'_>) -> Result<Value, Error> {
    let judgment = std::env::var_os("ITGMANIA_SONG_LUA_JUDGMENT").map(PathBuf::from);
    evaluate_with_resources(
        entries,
        context,
        Some(reference_noteskin()?),
        judgment.as_deref(),
    )
}

#[cfg(test)]
pub(crate) fn evaluate_with_noteskin(
    entries: &[Entry],
    context: &Context<'_>,
    noteskin: Option<crate::noteskin_oracle::Document>,
) -> Result<Value, Error> {
    evaluate_with_resources(entries, context, noteskin, None)
}

fn evaluate_with_resources(
    entries: &[Entry],
    context: &Context<'_>,
    noteskin: Option<crate::noteskin_oracle::Document>,
    judgment: Option<&Path>,
) -> Result<Value, Error> {
    if context.random_seed == 0 || context.random_seed > i32::MAX as u32 {
        return Err(Error::Request(
            "random seed must be in 1..=2147483647".into(),
        ));
    }
    for (name, value) in [
        ("max beat", context.max_beat),
        ("BPM", context.bpm),
        ("beat step", context.beat_step),
    ] {
        if !value.is_finite() || value <= 0.0 {
            return Err(Error::Request(format!(
                "{name} must be finite and positive"
            )));
        }
    }
    let request = encode(entries, context)?;
    let mut host = String::with_capacity(PLAYER_OPTION_METHODS.len() + HOST.len());
    host.push_str(PLAYER_OPTION_METHODS);
    host.push_str(&format!(
        "\n_ITG_SONG_RANDOM_SEED = {}\n",
        context.random_seed
    ));
    let judgment_bytes = judgment
        .map(|path| {
            fs::read(path).map_err(|error| {
                Error::Request(format!("read initial judgment {}: {error}", path.display()))
            })
        })
        .transpose()?;
    if let Some(path) = judgment {
        host.push_str(&format!(
            "\n_ITG_SONG_JUDGMENT = {}\n",
            crate::song_lua_runtime::lua_quote(&path.to_string_lossy())
        ));
    }
    if let Some(document) = &noteskin {
        use crate::song_lua_runtime::lua_quote;
        use std::fmt::Write;
        let _ = writeln!(
            host,
            "\n_ITG_SONG_NOTESKIN = {{ root = {}, skin = {}, metrics = {{",
            lua_quote(&document.noteskins_root),
            lua_quote(&document.skin)
        );
        for metric in &document.metrics {
            let key = format!("{}/{}", metric.section, metric.key).to_ascii_lowercase();
            let _ = writeln!(
                host,
                "[{}] = {{raw={}, number={}, boolean={}}},",
                lua_quote(&key),
                lua_quote(&metric.raw),
                metric.float.get(),
                metric.boolean
            );
        }
        host.push_str("}, paths = {\n");
        for path in &document.paths {
            if let Some(value) = &path.path {
                let key = format!("{}/{}", path.button, path.element).to_ascii_lowercase();
                let physical = value
                    .strip_prefix("NoteSkins/")
                    .map(|path| {
                        Path::new(&document.noteskins_root)
                            .join(path)
                            .to_string_lossy()
                            .into_owned()
                    })
                    .unwrap_or_else(|| value.clone());
                let _ = writeln!(host, "[{}] = {},", lua_quote(&key), lua_quote(&physical));
            }
        }
        host.push_str("}}\n");
    }
    host.push_str("\n_ITG_THEME_HELPERS = {\n");
    for (name, source) in THEME_HELPERS {
        host.push_str(&format!(
            "{{name={}, source={}}},\n",
            crate::song_lua_runtime::lua_quote(name),
            crate::song_lua_runtime::lua_quote(source)
        ));
    }
    host.push_str("}\n");
    // Keep only declared fallback helper names; arbitrary probes must be nil.
    host.push_str("\n_ITG_ACTOR_HELPERS = {}\n");
    for line in ACTOR_HELPERS.lines().map(str::trim) {
        let Some((name, _)) = line.strip_prefix("function ").and_then(|line| line.split_once('(')) else { continue };
        let Some((class, method)) = name.split_once(':') else { continue };
        let class = crate::song_lua_runtime::lua_quote(class);
        let method = crate::song_lua_runtime::lua_quote(method);
        host.push_str(&format!("_ITG_ACTOR_HELPERS[{class}] = _ITG_ACTOR_HELPERS[{class}] or {{}}\n_ITG_ACTOR_HELPERS[{class}][{method}] = true\n"));
    }
    host.push_str(HOST);
    let response = native_eval(&request, host.as_bytes())?;
    let mut input = Reader::new(&response);
    if input.take(MAGIC.len())? != MAGIC {
        return Err(Error::Wire("invalid native response magic".into()));
    }
    let version = input.u32()?;
    if version != WIRE_VERSION {
        return Err(Error::Wire(format!(
            "unsupported native wire version {version}"
        )));
    }
    match input.byte()? {
        0 => {}
        1 => return Err(Error::Native(input.string()?)),
        status => return Err(Error::Wire(format!("invalid native status {status}"))),
    }
    let json = input.bytes()?;
    if !input.is_empty() {
        return Err(Error::Wire("native response has trailing bytes".into()));
    }
    let mut document: Value = serde_json::from_slice(json).map_err(Error::Json)?;
    if let Some(path) = std::env::var_os("ITGMANIA_SONG_LUA_RAW_TRACE") {
        fs::write(path, json)
            .map_err(|error| Error::Request(format!("write diagnostic raw trace: {error}")))?;
    }
    crate::song_lua_semantics::enrich(&mut document).map_err(Error::Semantics)?;
    if let (Some(path), Some(bytes)) = (judgment, judgment_bytes) {
        use sha2::{Digest, Sha256};
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Request("initial judgment must have a UTF-8 filename".into()))?;
        let physical = path.to_string_lossy().replace('\\', "/");
        let physical = physical.trim_start_matches("//?/");
        let portable = format!("judgment:/{name}");
        normalize_judgment_path(&mut document, physical, &portable);
        document["judgment_reference"] = serde_json::json!({
            "path": name,
            "bytes": bytes.len(),
            "sha256": Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>()
        });
    }
    document["theme_reference"] = serde_json::json!({"root":"theme:/_fallback", "files": THEME_HELPERS.iter().map(|(name, source)| {
        use sha2::{Digest, Sha256};
        serde_json::json!({"path":format!("Scripts/{name}"), "bytes":source.len(), "sha256":Sha256::digest(source.as_bytes()).iter().map(|byte|format!("{byte:02x}")).collect::<String>()})
    }).collect::<Vec<_>>()});
    if let Some(noteskin) = noteskin {
        // Keep bundled noteskin dependencies portable and pin their bytes in
        // the trace. Whole-song archives list these as external resources.
        let root = noteskin.noteskins_root.replace('\\', "/");
        let prefix = format!("{}/", root.trim_end_matches('/'));
        let mut files = std::collections::BTreeSet::new();
        for path in noteskin
            .paths
            .iter()
            .filter_map(|path| path.path.as_deref())
        {
            if let Some(relative) = path.strip_prefix("NoteSkins/") {
                files.insert(relative.to_owned());
            }
        }
        normalize_noteskin_paths(&mut document, &prefix, &mut files);
        document["noteskin_reference"] = serde_json::to_value(&noteskin).map_err(Error::Json)?;
        document["noteskin_reference"]["noteskins_root"] = "noteskin:/".into();
        let mut hashes = Vec::new();
        for relative in files {
            use sha2::{Digest, Sha256};
            let path = Path::new(&root).join(&relative);
            let bytes = fs::read(&path).map_err(|error| {
                Error::Request(format!(
                    "read noteskin dependency {}: {error}",
                    path.display()
                ))
            })?;
            hashes.push(serde_json::json!({"path": relative, "bytes": bytes.len(), "sha256": Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>()}));
        }
        document["noteskin_reference"]["files"] = hashes.into();
    }
    Ok(document)
}

fn normalize_judgment_path(value: &mut Value, physical: &str, portable: &str) {
    match value {
        Value::String(path) if path.replace('\\', "/").trim_start_matches("//?/") == physical => {
            *path = portable.to_owned();
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| normalize_judgment_path(value, physical, portable)),
        Value::Object(values) => values
            .values_mut()
            .for_each(|value| normalize_judgment_path(value, physical, portable)),
        _ => {}
    }
}

fn normalize_noteskin_paths(
    value: &mut Value,
    prefix: &str,
    files: &mut std::collections::BTreeSet<String>,
) {
    match value {
        Value::String(path) => {
            let normalized = path.replace('\\', "/");
            if let Some(relative) = normalized.strip_prefix(prefix) {
                if Path::new(&normalized).is_file() {
                    files.insert(relative.to_owned());
                }
                *path = format!("noteskin:/{relative}");
            }
        }
        Value::Array(values) => values
            .iter_mut()
            .for_each(|value| normalize_noteskin_paths(value, prefix, files)),
        Value::Object(values) => values
            .values_mut()
            .for_each(|value| normalize_noteskin_paths(value, prefix, files)),
        _ => {}
    }
}

fn reference_noteskin() -> Result<crate::noteskin_oracle::Document, Error> {
    let root = std::env::var_os("ITGMANIA_SONG_LUA_NOTESKIN_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("ITGMANIA_BUILD_ROOT")).join("NoteSkins"));
    let skin = std::env::var("ITGMANIA_SONG_LUA_NOTESKIN").unwrap_or_else(|_| "cyber".into());
    let mut document = crate::noteskin_baseline::probe(Path::new(&root), "dance", &skin)
        .map_err(|error| Error::Request(format!("native song noteskin: {error}")))?;
    // Model templates use an underscore-prefixed button that is not a dance
    // input column, so it is outside the standard button inventory probe.
    let model = crate::noteskin_oracle::load(
        Path::new(&root),
        "dance",
        &skin,
        &[],
        &[crate::noteskin_oracle::PathQuery {
            button: "_down".into(),
            element: "tap note model".into(),
        }],
    )
    .map_err(|error| Error::Request(format!("native song noteskin model: {error}")))?;
    document.paths.extend(model.paths);
    Ok(document)
}

fn encode(entries: &[Entry], context: &Context<'_>) -> Result<Vec<u8>, Error> {
    let mut out = Writer::default();
    out.path(context.simfile)?;
    out.path(context.song_dir)?;
    out.string(context.title)?;
    out.string(context.difficulty)?;
    out.string(context.steps_type)?;
    out.string(context.description)?;
    out.string(env!("CARGO_PKG_VERSION"))?;
    out.string(env!("ITGMANIA_PRODUCT_VERSION"))?;
    out.f32(context.max_beat);
    out.f32(context.bpm);
    out.f32(context.beat_step);
    out.len(context.bpm_segments.len())?;
    for segment in context.bpm_segments {
        out.f32(segment.beat);
        out.f32(segment.bpm);
    }
    out.f32(854.0);
    out.f32(480.0);
    out.f32(854.0);
    out.f32(480.0);
    out.u32(context.max_events);
    out.u32(context.max_events.saturating_mul(16));
    out.u32(256);
    out.len(entries.len())?;
    for entry in entries {
        out.path(&entry.path)?;
        out.string(entry.layer)?;
        out.u32(entry.index);
        out.f32(entry.start_beat);
    }
    let texture_files = texture_files(context.song_dir)?;
    out.len(texture_files.len())?;
    for path in texture_files {
        out.path(&path)?;
    }
    Ok(out.bytes)
}

fn texture_files(root: &Path) -> Result<Vec<PathBuf>, Error> {
    let mut dirs = vec![root.to_path_buf()];
    let mut out = Vec::new();
    while let Some(dir) = dirs.pop() {
        let entries = fs::read_dir(&dir).map_err(|source| {
            Error::Request(format!(
                "could not read song directory {}: {source}",
                dir.display()
            ))
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| {
                Error::Request(format!(
                    "could not read song entry in {}: {source}",
                    dir.display()
                ))
            })?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|source| {
                Error::Request(format!(
                    "could not inspect song path {}: {source}",
                    path.display()
                ))
            })?;
            if file_type.is_dir() {
                dirs.push(path);
            } else if file_type.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        matches!(
                            ext.to_ascii_lowercase().as_str(),
                            "png" | "jpg" | "jpeg" | "bmp" | "gif" | "mp3" | "oga" | "ogg" | "wav"
                        )
                    })
            {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[derive(Default)]
struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn f32(&mut self, value: f32) {
        self.u32(value.to_bits());
    }

    fn len(&mut self, value: usize) -> Result<(), Error> {
        self.u32(
            value
                .try_into()
                .map_err(|_| Error::Request("native request exceeds wire limit".into()))?,
        );
        Ok(())
    }

    fn string(&mut self, value: &str) -> Result<(), Error> {
        self.len(value.len())?;
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn path(&mut self, value: &Path) -> Result<(), Error> {
        self.string(&value.to_string_lossy())
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], Error> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| Error::Wire("native response offset overflow".into()))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| Error::Wire("truncated native response".into()))?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(
            |_| Error::Wire("truncated native integer".into()),
        )?))
    }

    fn bytes(&mut self) -> Result<&'a [u8], Error> {
        let len = usize::try_from(self.u32()?)
            .map_err(|_| Error::Wire("native length does not fit usize".into()))?;
        self.take(len)
    }

    fn string(&mut self) -> Result<String, Error> {
        String::from_utf8(self.bytes()?.to_vec())
            .map_err(|_| Error::Wire("native response contains invalid UTF-8".into()))
    }
}

#[cfg(itgmania_oracle)]
fn native_eval(request: &[u8], host: &[u8]) -> Result<Vec<u8>, Error> {
    #[repr(C)]
    struct NativeBuffer {
        data: *mut u8,
        len: usize,
    }

    unsafe extern "C" {
        fn itg_oracle_eval_song_lua(
            request: *const u8,
            request_len: usize,
            host: *const u8,
            host_len: usize,
        ) -> NativeBuffer;
        fn itg_oracle_free(data: *mut u8);
    }

    // SAFETY: both byte slices remain alive for the duration of the call and
    // carry their explicit lengths.
    let native = unsafe {
        itg_oracle_eval_song_lua(request.as_ptr(), request.len(), host.as_ptr(), host.len())
    };
    if native.data.is_null() {
        return Err(Error::Native(
            "native song Lua semantic oracle returned no response".into(),
        ));
    }
    // SAFETY: the bridge returns `len` initialized bytes owned by `data` and
    // keeps them valid until the matching free call below.
    let bytes = unsafe { std::slice::from_raw_parts(native.data, native.len) }.to_vec();
    // SAFETY: `data` came from the bridge and has not previously been freed.
    unsafe { itg_oracle_free(native.data) };
    Ok(bytes)
}

#[cfg(not(itgmania_oracle))]
fn native_eval(_request: &[u8], _host: &[u8]) -> Result<Vec<u8>, Error> {
    Err(Error::Unavailable)
}

#[derive(Debug)]
pub enum Error {
    Request(String),
    Native(String),
    Wire(String),
    Json(serde_json::Error),
    Semantics(crate::song_lua_semantics::Error),
    #[cfg(not(itgmania_oracle))]
    Unavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(message) => formatter.write_str(message),
            Self::Native(message) => write!(formatter, "ITGmania Lua host failed: {message}"),
            Self::Wire(message) => write!(formatter, "invalid native semantic response: {message}"),
            Self::Json(error) => write!(
                formatter,
                "native semantic host returned invalid JSON: {error}"
            ),
            Self::Semantics(error) => {
                write!(formatter, "could not derive actor semantics: {error}")
            }
            #[cfg(not(itgmania_oracle))]
            Self::Unavailable => write!(
                formatter,
                "the embedded ITGmania song Lua oracle is supported on Windows and Linux/WSL only"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(itgmania_oracle)]
    #[test]
    fn actor_definitions_keep_native_concat_commands() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("definition fixture folder");
        let entry = song_dir.join("definition-concat.lua");
        let context = Context {
            simfile: &entry, song_dir: &song_dir, title: "Native definition concatenation",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
            max_beat: 0.2, bpm: 60.0, bpm_segments: &[], beat_step: 0.1,
            max_events: 1000, random_seed: 1,
        };
        let trace = evaluate_with_noteskin(
            &[Entry { path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0 }],
            &context, None,
        ).expect("definition capture");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn value_iterator_keeps_native_lookup() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("value iterator fixture folder");
        let entry = song_dir.join("value-iterator.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Native value iterator",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.2,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.1,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate_with_noteskin(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
            None,
        )
        .expect("value iterator capture");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn broadcasts_use_native_subscribers_and_preserve_params() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("broadcast fixture folder");
        let entry = song_dir.join("broadcast-subscribers.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Native broadcast subscribers",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.2,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.1,
            max_events: 1000,
            random_seed: 1,
        };
        // Each session must unsubscribe before closing its own Lua VM. Native
        // allocations can change order between sessions; require the observed
        // pointer order, never a fixed parent/child result.
        for _ in 0..4 {
            let trace = evaluate_with_noteskin(
                &[Entry {
                    path: entry.clone(),
                    layer: "foreground",
                    index: 0,
                    start_beat: 0.0,
                }],
                &context,
                None,
            )
            .expect("native subscriber capture");
            assert_eq!(trace["runtime_errors"], serde_json::json!([]));
            assert_eq!(trace["dropped_events"], 0);
            assert_eq!(trace["message_dispatch"], "native-subscriber-pointer-order");
            let actors = trace["runtime_actors"].as_array().expect("actors");
            let external = trace["external_actors"].as_array().expect("external actors");
            let rank = |id: &Value| {
                actors
                    .iter()
                    .chain(external)
                    .find(|actor| actor["id"] == *id)
                    .expect("subscribed runtime actor")["message_order"]
                    .as_u64()
                    .expect("native pointer rank")
            };
            let dispatches = trace["message_dispatches"]
                .as_array()
                .expect("native dispatches");
            assert_eq!(dispatches.iter().filter(|dispatch| dispatch["name"] == "Go").count(), 1,
                "rejected calls must not enter the broadcast trace");
            let go = dispatches
                .iter()
                .find(|dispatch| dispatch["name"] == "Go")
                .expect("Go broadcast");
            assert_eq!(go["actor_ids"].as_array().expect("Go subscribers").len(), 2);
            for dispatch in dispatches {
                let ids = dispatch["actor_ids"].as_array().expect("subscriber ids");
                assert!(ids.windows(2).all(|pair| rank(&pair[0]) < rank(&pair[1])));
                assert!(!ids.iter().any(|id| id == "def-0003"));
            }
            for name in ["Dynamic", "Outer", "Inner", "External", "Queued"] {
                let dispatch = dispatches
                    .iter()
                    .find(|dispatch| dispatch["name"] == name)
                    .expect("dynamic, nested or queued broadcast");
                assert_eq!(dispatch["actor_ids"].as_array().expect("subscribers").len(), 1);
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn recovered_host_bindings_and_recurring_commands() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        for name in ["host-bindings.lua", "recurring-command.lua"] {
            let path = dir.join(name);
            let context = Context {
                simfile: &path,
                song_dir: &dir,
                title: "Host regression",
                difficulty: "Difficulty_Challenge",
                steps_type: "dance-single",
                description: "",
                max_beat: 1.0,
                bpm: 60.0,
                bpm_segments: &[],
                beat_step: 0.25,
                max_events: 1000,
                random_seed: 1,
            };
            let trace = evaluate_with_noteskin(
                &[Entry {
                    path: path.clone(),
                    layer: "foreground",
                    index: 0,
                    start_beat: 0.0,
                }],
                &context,
                None,
            )
            .expect("evaluate recovered host APIs");
            assert_eq!(trace["runtime_errors"], serde_json::json!([]), "{name}");
            assert_eq!(trace["dropped_events"], 0, "{name}");
            assert_eq!(trace["end_position"]["seconds"], 1.0);
            assert_eq!(trace["trace_until_seconds"], 1.0);
            if name == "host-bindings.lua" {
                assert!(
                    trace["loaded_lua_files"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|p| p == "song:/host-child.lua")
                );
                assert!(trace["file_reads"].as_array().unwrap().iter().any(|read| read["path"] == "song:/host-data.txt" && read["exists"] == true));
                assert!(!dir.join("forbidden.txt").exists());
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn zero_fov_resets_native_parent_perspective() {
        let song_dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/song-lua-headless");
        let entry = song_dir.join("zero-fov.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "native zero FOV",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.05,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.01,
            max_events: 10000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native FOV control");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        for (name, fov) in [
            ("Zero", 0.0),
            ("Inherited", 60.0),
            ("Negative", 0.1),
            ("Small", 0.1),
            ("WrongCase", 60.0),
        ] {
            let actor = trace["runtime_actors"]
                .as_array().expect("native actors")
                .iter().find(|actor| actor["name"] == name).expect("FOV probe");
            let track = trace["projected_vertex_tracks"]
                .as_array().expect("native vertices")
                .iter().find(|track| track["actor"] == actor["id"]).expect("probe vertices");
            let sample = &track["samples"][0];
            assert!(
                (sample[7][0].as_f64().expect("native camera FOV") - fov).abs() < 1e-7,
                "{name}"
            );
            if name == "Zero" {
                for (world, screen) in sample[4].as_array().expect("world corners").iter()
                    .zip(sample[6].as_array().expect("screen corners"))
                {
                    for axis in 0..2 {
                        assert!(
                            (world[axis].as_f64().expect("world coordinate")
                                - screen[axis].as_f64().expect("screen coordinate"))
                            .abs() < 0.0001
                        );
                    }
                }
                for clip in sample[5].as_array().expect("clip corners") {
                    assert_eq!(clip[3], 1.0);
                    assert!((clip[2].as_f64().expect("clip depth") + 0.125).abs() < 1e-7);
                }
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn streams_large_semantic_document() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/song-lua-headless");
        let path = dir.join("semantic-json-stream.lua");
        let context = Context {
            simfile: &path,
            song_dir: &dir,
            title: "JSON \"stream\"\n\\",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.05,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.0125,
            max_events: 10_000,
            random_seed: 1,
        };
        let trace = evaluate_with_noteskin(
            &[Entry {
                path: path.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
            None,
        )
        .expect("stream multiple JSON chunks");
        assert!(serde_json::to_vec(&trace).expect("trace bytes").len() > 65_536 * 4);
        assert_eq!(trace["title"], context.title);
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let definitions = trace["actor_definitions"].as_array().expect("definitions");
        assert_eq!(definitions.len(), 401);
        assert_eq!(
            trace["projected_vertex_tracks"]
                .as_array()
                .expect("tracks")
                .len(),
            400
        );
        for index in 1..=400 {
            assert_eq!(definitions[index]["name"], format!("tile:{index}\n\"\\"));
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn proxy_methods_match_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root.join("tests/fixtures/song-lua-headless")
            .canonicalize().expect("native proxy fixtures");
        let entry = song_dir.join("proxy-methods.lua");
        let native_input: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("fixtures/actors/proxy-methods.json"))
                .expect("native proxy input"),
        ).expect("native proxy JSON");
        assert_eq!(std::fs::read_to_string(&entry).expect("semantic proxy assertions").replace("\r\n", "\n"),
            native_input["lua_assertions"].as_str().expect("native proxy assertions"));
        let context = Context {
            simfile: &entry, song_dir: &song_dir, title: "Native proxy methods",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
            max_beat: 4.0, bpm: 120.0, bpm_segments: &[], beat_step: 0.25,
            max_events: 1000, random_seed: 1,
        };
        let trace = evaluate_with_noteskin(&[Entry {
            path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
        }], &context, None).expect("native proxy semantic control");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native = crate::actor_conformance::evaluate(&root.join("fixtures/actors/proxy-methods.json"))
            .expect("same assertions on native userdata");
        assert_eq!(native["script_errors"], serde_json::json!([]));
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn sprite_methods_match_native() {
        for name in ["sprite-load", "texture-path"] {
            let root = Path::new(env!("CARGO_MANIFEST_DIR"));
            let song_dir = root
                .join("tests/fixtures/song-lua-headless")
                .canonicalize()
                .expect("native Sprite fixtures");
            let entry = song_dir.join(format!("{name}.lua"));
            let input = root.join(format!("fixtures/actors/{name}.json"));
            let native_input: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&input).expect("native Sprite input"))
                    .expect("native Sprite JSON");
            let body = native_input["lua_assertions"]
                .as_str()
                .expect("native Sprite assertions")
                .split_once('\n')
                .expect("fixture path boundary")
                .1;
            assert!(
                std::fs::read_to_string(&entry)
                    .expect("runtime Sprite assertions")
                    .replace("\r\n", "\n")
                    .contains(body)
            );
            let native = crate::actor_conformance::evaluate(&input)
                .expect("same assertions on compiled Sprite userdata");
            assert_eq!(native["script_errors"], serde_json::json!([]));
            let context = Context {
                simfile: &entry,
                song_dir: &song_dir,
                title: "Native Sprite loading",
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
                    path: entry.clone(),
                    layer: "foreground",
                    index: 0,
                    start_beat: 0.0,
                }],
                &context,
                None,
            )
            .expect("native Sprite semantic control");
            assert_eq!(trace["runtime_errors"], serde_json::json!([]));
            assert_eq!(trace["dropped_events"], 0);
            if name == "texture-path" {
                assert!(trace["events"].as_array().expect("events").iter()
                    .any(|event| event["operation"] == "Sprite.Load"
                        && event["args"][0] == "song:/./fit-rect.png"),
                    "record portable asset references without changing native Lua filenames");
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn runtime_actor_contracts() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root.join("tests/fixtures/song-lua-headless")
            .canonicalize().expect("native runtime actor fixtures");
        for name in ["actor-string", "bitmap-bools"] {
            let entry = song_dir.join(format!("{name}.lua"));
            let input = root.join(format!("fixtures/actors/{name}.json"));
            let native_input: serde_json::Value = serde_json::from_slice(
                &std::fs::read(&input).expect("native runtime actor input"),
            ).expect("native runtime actor JSON");
            let body = native_input["lua_assertions"].as_str()
                .expect("native runtime actor assertions");
            assert!(std::fs::read_to_string(&entry).expect("runtime actor assertions")
                .replace("\r\n", "\n").contains(body));
            let native = crate::actor_conformance::evaluate(&input)
                .expect("same assertions on compiled native userdata");
            for key in ["script_errors", "diagnostics", "allocations"] {
                assert_eq!(native[key], serde_json::json!([]), "{name}: {key}");
            }
            let context = Context {
                simfile: &entry, song_dir: &song_dir, title: "Native runtime actor contracts",
                difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
                max_beat: 4.0, bpm: 120.0, bpm_segments: &[], beat_step: 0.25,
                max_events: 1000, random_seed: 1,
            };
            let trace = evaluate_with_noteskin(&[Entry {
                path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
            }], &context, None).expect("native runtime actor semantic control");
            assert_eq!(trace["runtime_errors"], serde_json::json!([]), "{name}");
            assert_eq!(trace["dropped_events"], 0, "{name}");
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn bitmap_methods_match_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root.join("tests/fixtures/song-lua-headless")
            .canonicalize().expect("native bitmap fixtures");
        let entry = song_dir.join("bitmap-methods.lua");
        let input = root.join("fixtures/actors/bitmap-methods.json");
        let native_input: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&input).expect("native bitmap input"),
        ).expect("native bitmap JSON");
        // The native oracle checks raw inventory, before fallback Lua helpers.
        let assertions = native_input["lua_assertions"].as_str()
            .expect("native bitmap assertions").split_once("assert(type(BitmapText.GetX)")
            .expect("native inventory boundary").1;
        assert!(std::fs::read_to_string(&entry).expect("semantic bitmap assertions")
            .replace("\r\n", "\n").contains(&format!("assert(type(BitmapText.GetX){assertions}")));
        let context = Context {
            simfile: &entry, song_dir: &song_dir, title: "Native bitmap methods",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
            max_beat: 4.0, bpm: 120.0, bpm_segments: &[], beat_step: 0.25,
            max_events: 1000, random_seed: 1,
        };
        let trace = evaluate_with_noteskin(&[Entry {
            path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
        }], &context, None).expect("native bitmap semantic control");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native = crate::actor_conformance::evaluate(&input)
            .expect("same assertions on native BitmapText userdata");
        assert_eq!(native["script_errors"], serde_json::json!([]));
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn runtime_class_methods_match_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root.join("tests/fixtures/song-lua-headless")
            .canonicalize().expect("native runtime class fixtures");
        let entry = song_dir.join("bitmap-runtime-methods.lua");
        let input = root.join("fixtures/actors/bitmap-runtime-methods.json");
        let native_input: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&input).expect("native runtime class input"),
        ).expect("native runtime class JSON");
        let script = std::fs::read_to_string(&entry).expect("runtime class assertions")
            .replace("\r\n", "\n");
        let body = script.strip_suffix("\nreturn Def.ActorFrame{}\n")
            .expect("runtime class assertion boundary");
        assert!(native_input["lua_assertions"].as_str()
            .expect("native runtime class assertions").ends_with(body));
        let context = Context {
            simfile: &entry, song_dir: &song_dir, title: "Native runtime class methods",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
            max_beat: 4.0, bpm: 120.0, bpm_segments: &[], beat_step: 0.25,
            max_events: 1000, random_seed: 1,
        };
        let trace = evaluate_with_noteskin(&[Entry {
            path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
        }], &context, None).expect("native runtime class semantic control");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native = crate::actor_conformance::evaluate(&input)
            .expect("runtime class assertions on native method tables");
        assert_eq!(native["script_errors"], serde_json::json!([]));
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn inherited_class_values_match_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root.join("tests/fixtures/song-lua-headless")
            .canonicalize().expect("native class value fixtures");
        let entry = song_dir.join("actor-class-values.lua");
        let input = root.join("fixtures/actors/actor-class-values.json");
        let native_input: serde_json::Value = serde_json::from_slice(
            &std::fs::read(&input).expect("native class value input"),
        ).expect("native class value JSON");
        let body = native_input["lua_assertions"].as_str().expect("native class value assertions");
        assert_eq!(std::fs::read_to_string(&entry).expect("semantic class value assertions")
            .replace("\r\n", "\n"), format!("{body}\nreturn Def.ActorFrame{{}}\n"));
        let context = Context {
            simfile: &entry, song_dir: &song_dir, title: "Native inherited class values",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
            max_beat: 4.0, bpm: 120.0, bpm_segments: &[], beat_step: 0.25,
            max_events: 1000, random_seed: 1,
        };
        let trace = evaluate_with_noteskin(&[Entry {
            path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
        }], &context, None).expect("native inherited class value control");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native = crate::actor_conformance::evaluate(&input)
            .expect("same assertions on native class tables");
        assert_eq!(native["script_errors"], serde_json::json!([]));
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn loads_relative_actor_from_queued_command() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("native fixtures");
        let entry = song_dir.join("late-load/default.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Late load",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.3,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.125,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate_with_noteskin(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
            None,
        )
        .expect("queued relative actor");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert!(
            trace["loaded_lua_files"]
                .as_array()
                .expect("sources")
                .iter()
                .any(|path| path == "song:/late-load/helper.lua")
        );
        let event = trace["events"]
            .as_array()
            .expect("events")
            .iter()
            .find(|event| event["operation"] == "Quad.x")
            .expect("loaded helper value");
        assert_eq!(event["args"][0], 432);
        assert!(event["seconds"].as_f64().expect("dispatch time") >= 0.1);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn loads_audio_as_sound_actors() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("native fixtures");
        let entry = song_dir.join("load-sound.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Load sound",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.1,
            bpm: 120.0,
            bpm_segments: &[],
            beat_step: 0.125,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate_with_noteskin(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
            None,
        )
        .expect("native audio actors");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        for (index, name, class) in [
            (1, "Extensionless", "Sound"),
            (2, "Explicit", "Sound"),
            (3, "Image", "Sprite"),
        ] {
            assert_eq!(trace["actor_definitions"][index]["name"], name);
            assert_eq!(trace["actor_definitions"][index]["class"], class);
        }
        assert_eq!(
            trace["actor_definitions"][1]["properties"]["File"],
            "song:/sound-clip.wav"
        );
        assert_eq!(
            trace["actor_definitions"][2]["properties"]["File"],
            "song:/sound-clip.wav"
        );
        assert!(
            trace["projected_vertex_tracks"]
                .as_array()
                .expect("geometry")
                .iter()
                .all(|track| track["class"] == "Sprite")
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn captures_requested_random_seed() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("native fixture directory");
        let entry = song_dir.join("random-seed.lua");
        let entries = [Entry {
            path: entry.clone(),
            layer: "foreground",
            index: 0,
            start_beat: 0.0,
        }];
        let mut context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Capture random seed",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.1,
            bpm: 120.0,
            bpm_segments: &[],
            beat_step: 0.125,
            max_events: 1000,
            random_seed: 1,
        };
        for (seed, values) in [
            (1, [2401, 5741, 4147, 5369, 0]),
            (2, [2510, 1065, 149, 5363, 3164]),
        ] {
            context.random_seed = seed;
            let trace =
                evaluate_with_noteskin(&entries, &context, None).expect("native random seed");
            assert_eq!(trace["runtime_errors"], serde_json::json!([]));
            assert_eq!(trace["random_seed"], seed);
            for (operation, value) in [
                "Quad.x",
                "Quad.y",
                "Quad.z",
                "Quad.rotationz",
                "Quad.diffusealpha",
            ]
            .into_iter()
            .zip(values)
            {
                let event = trace["events"]
                    .as_array()
                    .expect("events")
                    .iter()
                    .find(|event| event["operation"] == operation)
                    .expect("random draw");
                assert_eq!(event["args"][0], value);
            }
        }
        for seed in [0, u32::MAX] {
            context.random_seed = seed;
            assert!(matches!(
                evaluate_with_noteskin(&entries, &context, None),
                Err(Error::Request(_))
            ));
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn shared_definitions_create_separate_actors() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("song fixture folder");
        let entry = song_dir.join("shared-actor.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Shared actor definition",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.3,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let mut trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native shared actor instances");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let actors = trace["runtime_actors"].as_array().expect("runtime actors");
        let ids = actors
            .iter()
            .map(|actor| actor["id"].as_str().expect("actor ID"))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            ids.len(),
            actors.len(),
            "runtime IDs must identify instances"
        );
        for (id, parent, alpha) in [
            ("def-0003", "def-0002", 0.25),
            ("def-0003#2", "def-0004", 0.75),
        ] {
            let actor = actors
                .iter()
                .find(|actor| actor["id"] == id)
                .expect("shared instance");
            assert_eq!(actor["parent_id"], parent);
            assert_eq!(actor["final_render_state"]["alpha"], alpha);
            let track = trace["projected_vertex_tracks"]
                .as_array()
                .expect("geometry tracks")
                .iter()
                .find(|track| track["actor"] == id)
                .expect("instance geometry");
            assert_eq!(track["samples"][0][3], alpha);
        }
        crate::song_lua_semantics::enrich(&mut trace).expect("instance-aware semantics");
        for (parent, child) in [("def-0002", "def-0003"), ("def-0004", "def-0003#2")] {
            let order = trace["draw_orders"]
                .as_array()
                .expect("draw orders")
                .iter()
                .find(|order| order["parent_actor"] == parent)
                .expect("parent draw order");
            assert_eq!(order["final_children"][0]["actor"], child);
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn foreground_init_precedes_next_file() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("song fixture folder");
        let first = song_dir.join("load-first.lua");
        let context = Context {
            simfile: &first, song_dir: &song_dir, title: "Foreground load order",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
            max_beat: 0.2, bpm: 60.0, bpm_segments: &[], beat_step: 0.1,
            max_events: 1000, random_seed: 1,
        };
        // Foreground.cpp:36-62 loads and initializes each actor before the next
        // file. ActorFrame.cpp:89-94 initializes children before the frame.
        let trace = evaluate(
            &[
                Entry { path: first.clone(), layer: "foreground", index: 0, start_beat: 0.0 },
                Entry { path: song_dir.join("load-second.lua"), layer: "foreground", index: 1, start_beat: 0.1 },
            ], &context,
        ).expect("foreground capture");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        assert_eq!(trace["roots"].as_array().expect("roots").len(), 2);
        assert_eq!(trace["actor_definitions"].as_array().expect("definitions").len(), 3);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn init_queues_wait_for_on_commands() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("song fixture folder");
        let entry = song_dir.join("init-queue.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Init queue order",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.3,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native Init queue order");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert!(
            trace["events"]
                .as_array()
                .expect("events")
                .iter()
                .any(|event| event["operation"] == "Actor.x"
                    && event["args"] == serde_json::json!([23]))
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn timing_preference_round_trips_native_float() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("song fixture folder");
        let entry = song_dir.join("timing-preference.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "timing preference",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.3,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("mutable native float preference");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn initial_update_uses_loading_budget() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("song fixture folder");
        let context = Context {
            simfile: &song_dir,
            song_dir: &song_dir,
            title: "initial loading budget",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 200.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 1.0,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: song_dir.join("initial-budget.lua"),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("finite initial update");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["end_position"]["beat"], 200.0);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn instruction_guard_bounds_each_replay_frame() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("song fixture folder");
        let entry = song_dir.join("frame-budget.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "frame budget",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 2.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 1.0,
            max_events: 1000,
            random_seed: 1,
        };
        let layer = |path| Entry {
            path,
            layer: "foreground",
            index: 0,
            start_beat: 0.0,
        };
        // Its finite callbacks exceed the former song-wide 200M minimum,
        // while every individual frame remains well within that bound.
        let trace = evaluate(&[layer(entry.clone())], &context)
            .expect("finite frames must finish regardless of render sample spacing");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["end_position"]["beat"], 2.0);
        let runaway = evaluate(&[layer(song_dir.join("runaway-frame.lua"))], &context)
            .expect("capture the bounded callback failure");
        let errors = runaway["runtime_errors"]
            .as_array()
            .expect("runtime errors");
        assert_eq!(errors.len(), 1);
        assert!(
            errors[0]["message"]
                .as_str()
                .expect("instruction error")
                .contains("instruction budget exhausted at beat 1"),
            "{errors:?}"
        );
        assert_eq!(runaway["capabilities"]["runtime_complete"], false);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn song_steps_keep_native_order_and_current_identity() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("song fixture folder");
        let simfile = song_dir.join("song-steps.sm");
        let entry = song_dir.join("song-steps.lua");
        let context = Context {
            simfile: &simfile,
            song_dir: &song_dir,
            title: "Song Steps",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "selected",
            max_beat: 0.3,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native song Steps capture");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(
            trace["actor_definitions"]
                .as_array()
                .expect("definitions")
                .len(),
            1
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    #[ignore = "requires the local mawaru9 song corpus"]
    fn mawaru9_startup_reads_current_chart() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../lua-songs/mawaru9")
            .canonicalize()
            .expect("Mawaru9 song folder");
        let simfile = song_dir.join("mawaru9.sm");
        let context = Context {
            simfile: &simfile,
            song_dir: &song_dir,
            title: "MAWARU SIMULATOR 2016",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "TaroNuke",
            max_beat: 0.3,
            bpm: 90.0,
            bpm_segments: &[],
            beat_step: 0.125,
            max_events: 200_000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: song_dir.join("lua/default.lua"),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("Mawaru9 startup capture");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);

        assert!(
            trace["actor_definitions"]
                .as_array()
                .expect("definitions")
                .iter()
                .any(|actor| actor["name"] == "actor_yp")
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn update_rate_validation() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("headless fixtures");
        let entry = song_dir.join("update-rate-validation.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "update rate validation",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.5,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native update rate validation");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn hibernation_pauses_the_actor_tree() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        for (fixture, parent_alpha, child_alpha) in [
            ("hibernate.lua", 29.0 / 30.0, 29.0 / 30.0),
            ("hibernate-rate.lua", 14.0 / 15.0, 0.8),
        ] {
            let entry = song_dir.join(fixture);
            let bpms = [BpmSegment {
                beat: 0.0,
                bpm: 60.0,
            }];
            let context = Context {
                simfile: &entry,
                song_dir: &song_dir,
                title: "hibernation",
                difficulty: "Difficulty_Challenge",
                steps_type: "dance-single",
                description: "",
                max_beat: 0.5,
                bpm: 60.0,
                bpm_segments: &bpms,
                beat_step: 0.25,
                max_events: 1000,
                random_seed: 1,
            };
            let trace = evaluate(
                &[Entry {
                    path: entry.clone(),
                    layer: "foreground",
                    index: 0,
                    start_beat: 0.0,
                }],
                &context,
            )
            .expect("native hibernation trace");
            assert_eq!(trace["runtime_errors"], serde_json::json!([]));
            for actor in trace["runtime_actors"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|actor| actor["name"] == "Sleeping" || actor["name"] == "Child")
            {
                let samples = actor["render_state_samples"].as_array().unwrap();
                assert_eq!(samples[0], serde_json::json!([0, 1, true]));
                assert_eq!(samples[1][0], 8, "updates pause through frame seven");
                let expected = if actor["name"] == "Sleeping" {
                    parent_alpha
                } else {
                    child_alpha
                };
                assert!((samples[1][1].as_f64().unwrap() - expected).abs() < 0.000001);
                assert_eq!(
                    actor["final_render_state"],
                    serde_json::json!({"alpha": 0, "visible": true})
                );
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn final_state_includes_unsampled_update_writes() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("final-state.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "unsampled final state",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.3,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native final-state trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let target = trace["runtime_actors"].as_array().expect("runtime actors")
            .iter().find(|actor| actor["name"] == "FinalState").expect("final-state Quad");
        assert_eq!(
            target["final_render_state"],
            serde_json::json!({ "alpha": 0, "visible": false })
        );
        assert_eq!(
            target["render_state_samples"],
            serde_json::json!([[0, 1, true], [17, 0, false]])
        );
        assert_eq!(trace["update_frames"].as_array().unwrap().len(), 19);
        assert!((trace["update_frames"][17][0].as_f64().unwrap() - 17.0 / 60.0).abs() < 1e-9);
        let samples = trace["projected_vertex_tracks"][0]["samples"]
            .as_array()
            .expect("projected samples");
        assert_eq!(
            samples.last().unwrap()[2],
            true,
            "sampled geometry precedes the final hide"
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn particle_fade_records_each_update_frame() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("particle-fade.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 132.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "particle fade",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 16.0,
            bpm: 132.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native particle trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let samples = trace["runtime_actors"][1]["render_state_samples"]
            .as_array()
            .unwrap();
        for (frame, alpha, visible) in [
            (130, 1.0, true),
            (132, 0.92, true),
            (136, 0.76, true),
            (150, 0.20, true),
            (155, 0.0, false),
            (362, 1.0, true),
            (368, 0.76, true),
            (387, 0.0, false),
        ] {
            let sample = samples
                .iter()
                .find(|sample| sample[0].as_u64() == Some(frame))
                .unwrap_or_else(|| panic!("missing frame {frame}: {samples:?}"));
            assert!(
                (sample[1].as_f64().unwrap() - alpha).abs() < 1e-9,
                "frame {frame}: {sample}"
            );
            assert_eq!(sample[2], visible, "frame {frame}");
            assert!(
                (trace["update_frames"][frame as usize][0].as_f64().unwrap()
                    - frame as f64 * 132.0 / 3600.0)
                    .abs()
                    < 1e-9
            );
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn hidden_actor_tweens_drive_modifiers() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("signal-tween.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "signal tween",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 3.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native signal tween trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let samples = trace["events"].as_array().unwrap();
        for (beat, expected) in [
            (0.5, 0.0),
            (1.0, 0.0),
            (1.25, 0.25),
            (1.5, 1.0 / 3.0),
            (1.75, 1.0 / 3.0),
            (2.0, 7.0 / 12.0),
            (2.25, 2.0 / 3.0),
        ] {
            for (operation, expected) in [
                ("PlayerOptions.Invert", expected),
                ("PlayerOptions.Drunk", 0.0),
                ("PlayerOptions.Wave", 0.0),
            ] {
                let sample = samples
                    .iter()
                    .filter(|sample| sample["operation"] == operation)
                    .min_by(|a, b| {
                        (a["beat"].as_f64().unwrap() - beat)
                            .abs()
                            .total_cmp(&(b["beat"].as_f64().unwrap() - beat).abs())
                    })
                    .unwrap();
                assert!((sample["beat"].as_f64().unwrap() - beat).abs() < 1e-6);
                let actual = sample["args"][0].as_f64().unwrap();
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "{operation} at {beat}: {actual} != {expected}"
                );
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn public_music_seconds_matches_native_binding() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize().expect("native music clock fixtures");
        for name in ["song-position-positive", "song-position-negative"] {
            let simfile = song_dir.join(format!("{name}.sm"));
            let context = Context {
                simfile: &simfile, song_dir: &song_dir, title: name,
                difficulty: "Difficulty_Challenge", steps_type: "dance-single",
                description: "clock", max_beat: 2.0, bpm: 60.0,
                bpm_segments: &[], beat_step: 0.25, max_events: 10000, random_seed: 1,
            };
            let trace = evaluate_with_noteskin(&[Entry {
                path: song_dir.join(format!("{name}.lua")), layer: "foreground",
                index: 0, start_beat: 0.0,
            }], &context, None).expect("native SongPosition capture");
            assert_eq!(trace["runtime_errors"], serde_json::json!([]), "{name}");
            assert_eq!(trace["dropped_events"], 0);
            assert_eq!(trace["update_frames"].as_array().expect("native frames").len(), 121);
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn song_clock_uses_global_pauses() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("song-clock.lua");
        let simfile = song_dir.join("song-clock.ssc");
        let context = Context {
            simfile: &simfile,
            song_dir: &song_dir,
            title: "song clock",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "clock",
            max_beat: 7.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 1.0 / 240.0,
            max_events: 10000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry,
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["song_clock"], "native-song-timing");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let events = trace["events"].as_array().unwrap();
        for (second, invert, freeze, delay, speed, x) in [
            (1.2, 0.12, 0.0, 0.0, 1.0, 0.0),
            (2.3, 0.2, 1.0, 0.0, 1.0, 0.375),
            (2.4, 0.2, 1.0, 0.0, 1.0, 0.5),
            (3.6, 0.3, 0.0, 1.0, 1.0, 0.5),
            (4.0, 0.325, 0.0, 0.0, 1.0, 0.5),
            (4.8, 0.41, 0.0, 0.0, 2.0, 0.5),
            (5.0, 0.45, 0.0, 0.0, 2.0, 0.5),
            (5.5, 0.65, 0.0, 0.0, 2.0, 0.5),
        ] {
            for (operation, expected) in [
                ("Invert", invert),
                ("Drunk", freeze),
                ("Wave", delay),
                ("XMod", speed),
                ("Tornado", x),
            ] {
                let sample = events
                    .iter()
                    .filter(|event| event["operation"] == format!("PlayerOptions.{operation}"))
                    .min_by(|a, b| {
                        (a["seconds"].as_f64().unwrap() - second)
                            .abs()
                            .total_cmp(&(b["seconds"].as_f64().unwrap() - second).abs())
                    })
                    .unwrap();
                assert!(
                    (sample["seconds"].as_f64().unwrap() - second).abs() < 1e-6,
                    "missing {operation} at {second}: {sample}"
                );
                let actual = sample["args"][0].as_f64().unwrap();
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "{operation} at {second}: {actual} != {expected}"
                );
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn continuous_song_clock_uses_native_float() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("song-clock.lua");
        let simfile = song_dir.join("song-clock-continuous.sm");
        let context = Context {
            simfile: &simfile,
            song_dir: &song_dir,
            title: "continuous float clock",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "clock",
            max_beat: 0.1,
            bpm: 120.0,
            bpm_segments: &[],
            beat_step: 1.0 / 60.0,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry,
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["song_clock"], "native-song-timing");
        // SongPosition::UpdateSongPosition receives float music seconds;
        // TimingData multiplies by the float BPS even without timing pauses.
        for (frame, seconds) in [(1, 1.0_f64 / 60.0), (2, 2.0_f64 / 60.0)] {
            assert_eq!(
                trace["update_frames"][frame][0].as_f64().unwrap(),
                f64::from(seconds as f32 * 2.0)
            );
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn song_clock_retains_native_float_rounding() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("song-clock.lua");
        let simfile = song_dir.join("song-clock-float.ssc");
        let context = Context {
            simfile: &simfile,
            song_dir: &song_dir,
            title: "float song clock",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "clock",
            max_beat: 76.0,
            bpm: 44.5,
            bpm_segments: &[],
            beat_step: 1.0 / 240.0,
            max_events: 20000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry,
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let events = trace["events"].as_array().unwrap();
        for (seconds, beat, expected) in [
            (32.5, 74.58333587646484_f64, 7.458333492279053_f64),
            (32.75, 74.95417022705078, 7.49541711807251),
            (33.0, 75.32500457763672, 7.532500267028809),
        ] {
            let event = events
                .iter()
                .find(|event| {
                    event["operation"] == "PlayerOptions.Invert"
                        && (event["seconds"].as_f64().unwrap() - seconds).abs() < 1e-9
                })
                .expect("native float song-clock sample");
            assert_eq!(event["beat"].as_f64().unwrap(), beat);
            assert_eq!(event["args"][0].as_f64().unwrap() as f32, expected as f32);
        }
    }

    #[cfg(all(itgmania_oracle, target_os = "windows"))]
    #[test]
    fn seeded_random_matches_theme_startup() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("random.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "seeded random",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 500,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native seeded random trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        for (actor, operation, expected) in [
            ("def-0002", "Quad.x", -2.0),
            ("def-0002", "Quad.y", 0.7203244895202072),
            ("def-0002", "Quad.z", 1.0),
            ("def-0002", "Quad.zoom", 0.12812444777230592),
            ("def-0003", "Quad.x", 0.7856989287092829),
            ("def-0003", "Quad.y", 0.3580021715717493),
            ("def-0003", "Quad.z", 445541548.0),
            ("def-0004", "Quad.x", -2.0),
            ("def-0004", "Quad.y", 0.7203244895202072),
            ("def-0004", "Quad.z", 1.0),
            ("def-0004", "Quad.zoom", 0.12812444777230592),
        ] {
            let event = trace["events"]
                .as_array()
                .unwrap()
                .iter()
                .find(|event| event["actor"] == actor && event["operation"] == operation)
                .unwrap_or_else(|| panic!("missing {actor} {operation}"));
            assert_eq!(event["args"][0].as_f64(), Some(expected));
        }
        assert_eq!(trace["random_seed"], 1);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn aft_create_freezes_allocation_and_reports_failures() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("aft-create.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "AFT creation",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 2.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native AFT creation trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let errors = trace["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["operation"] == "ActorFrame.x")
            .map(|event| event["args"][0].as_f64().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(errors, [1.0, 2.0, 3.0]);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn aft_texture_name_preserves_actor_name() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("aft-identity.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "AFT identity",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 500,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native AFT identity trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["actor_definitions"][1]["name"], "ActorLabel");
        assert!(trace["actor_definitions"][3]["name"].is_null());
        let events = trace["events"].as_array().unwrap();
        for (actor, expected) in [("def-0006", 10.0), ("def-0007", 0.0)] {
            let event = events
                .iter()
                .find(|event| event["actor"] == actor && event["operation"] == "Quad.x")
                .unwrap();
            assert_eq!(event["args"][0], expected);
        }
        let texture_name = events
            .iter()
            .find(|event| event["operation"] == "ActorFrameTexture.SetTextureName")
            .unwrap();
        assert_eq!(texture_name["args"][0], "TextureLabel");
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn recurring_ease_tables_preserve_shared_state() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("recurring-ease-table.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Recurring Ease Table",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 4.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native recurring ease table trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let mut checks = 0;
        for track in trace["callback_operation_tracks"].as_array().unwrap() {
            if !matches!(track["operation"].as_str(), Some("ActorFrame.x" | "ActorFrame.y")) {
                continue;
            }
            for sample in track["samples"].as_array().unwrap() {
                let beat = sample[1].as_f64().unwrap();
                let expected = if beat < 1.0 {
                    0.0
                } else if track["operation"] == "ActorFrame.y" {
                    ((beat.min(3.0) - 1.0) * 60.0).round() + 1.0
                } else {
                    let (from, to) = if track["actor"] == "def-0002" {
                        (4.0, 12.0)
                    } else {
                        (20.0, 40.0)
                    };
                    let t = (beat.min(3.0) - 1.0) / 2.0;
                    from + (to - from) * t * t
                };
                assert!((sample[3][0].as_f64().unwrap() - expected).abs() < 1e-6);
                checks += 1;
            }
        }
        assert_eq!(checks, 72);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn spline_storage_uses_native_bounds_and_values() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
        let entry = song_dir.join("spline-storage.lua");
        let context = Context {
            simfile: &entry, song_dir: &song_dir, title: "Native spline storage",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
            max_beat: 1.0, bpm: 60.0, bpm_segments: &[], beat_step: 0.25,
            max_events: 10000, random_seed: 1,
        };
        let trace = evaluate(&[Entry {
            path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
        }], &context).unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        assert_eq!(trace["capabilities"]["native_column_splines"], true);
        let writes = trace["events"].as_array().expect("native setter events");
        for (operation, value) in [("ActorFrame.x", -2), ("ActorFrame.y", 8)] {
            assert!(writes.iter().any(|event| event["operation"] == operation
                && event["args"] == serde_json::json!([value])));
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn position_splines_record_native_updates() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("position-spline.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Position spline",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 4.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 10000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let tracks = trace["callback_operation_tracks"].as_array().unwrap();
        let points: usize = tracks
            .iter()
            .filter(|track| track["operation"] == "Spline.SetPoint")
            .map(|track| track["samples"].as_array().unwrap().len())
            .sum();
        // Eight retained quarter-beat frames, four points, two handlers.
        assert_eq!(points, 64);
        let reverse: Vec<_> = trace["events"]
            .as_array()
            .expect("native events")
            .iter()
            .filter(|event| event["operation"] == "PlayerOptions.Reverse")
            .collect();
        assert!(!reverse.is_empty(), "native Reverse writes");
        assert!(
            reverse
                .iter()
                .all(|event| (event["args"][0].as_f64().unwrap() - 0.35).abs() < 1e-6)
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn zoomto_tweens_match_native_actors() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("zoomto-tween.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "zoomto tween",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 2.5,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.025,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/zoomto-tween.json"))
                .unwrap();
        let samples = trace["projected_vertex_tracks"][0]["samples"]
            .as_array()
            .unwrap();
        for expected in native["samples"].as_array().unwrap() {
            let second = expected["time"].as_f64().unwrap();
            let actual = samples
                .iter()
                .rev()
                .find(|s| s[1].as_f64().unwrap() <= second + 1e-6)
                .unwrap();
            let vertices = expected["actors"][1]["draws"][0]["vertices"]
                .as_array()
                .unwrap();
            for actual in actual[6].as_array().unwrap() {
                assert!(
                    vertices
                        .iter()
                        .any(|expected| (0..2).all(|axis| (actual[axis].as_f64().unwrap()
                            - expected["screen"][axis].as_f64().unwrap())
                        .abs()
                            < 0.001)),
                    "second {second}: actual {actual}, native {vertices:?}"
                );
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn background_fit_and_smooth_match_native_actors() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("background-fit-smooth.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "background fit and smooth",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 4.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let native = crate::actor_conformance::evaluate(
            &root.join("fixtures/actors/background-fit-smooth.json"),
        )
        .unwrap();
        let definitions = trace["actor_definitions"].as_array().unwrap();
        let id_for = |name| {
            definitions
                .iter()
                .find(|actor| actor["name"] == name)
                .unwrap()["id"]
                .as_str()
                .unwrap()
        };
        let projected = trace["projected_vertex_tracks"].as_array().unwrap();
        let fit = projected
            .iter()
            .find(|track| track["definition_id"] == id_for("Fit"))
            .unwrap();
        let native_fit = &native["samples"][0]["actors"][2]["draws"][0]["vertices"];
        fn check_vertices(actual: &serde_json::Value, native: &serde_json::Value) {
            let actual = actual.as_array().unwrap();
            let native = native.as_array().unwrap();
            assert_eq!(actual.len(), 4);
            assert_eq!(native.len(), actual.len());
            for vertex in actual {
                assert!(
                    native.iter().any(|expected| (0..2).all(|axis| (vertex[axis]
                        .as_f64()
                        .unwrap()
                        - expected["screen"][axis].as_f64().unwrap())
                    .abs()
                        < 1e-4)),
                    "actual {vertex}, native {native:?}"
                );
            }
        }
        check_vertices(&fit["samples"][0][6], native_fit);
        let cover_track = projected
            .iter()
            .find(|track| track["definition_id"] == id_for("Cover"))
            .unwrap();
        let drawn_cover = cover_track["samples"]
            .as_array()
            .unwrap()
            .iter()
            .find(|sample| {
                sample[6]
                    .as_array()
                    .is_some_and(|vertices| !vertices.is_empty())
            })
            .unwrap();
        check_vertices(
            &drawn_cover[6],
            &native["samples"][1]["actors"][3]["draws"][0]["vertices"],
        );
        let cover = trace["runtime_actors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|actor| actor["definition_id"] == id_for("Cover"))
            .unwrap();
        let states = cover["render_state_samples"].as_array().unwrap();
        // The actor's own update enqueues the tween after its first 1/60s tick.
        for (frame, alpha, sample) in [(91, 0.15625, 1), (121, 0.5, 2), (151, 0.84375, 3)] {
            let state = states
                .iter()
                .rev()
                .find(|state| state[0].as_u64().unwrap() <= frame)
                .unwrap();
            let native_alpha = native["samples"][sample]["actors"][3]["current"]["diffuse"][0][3]
                .as_f64()
                .unwrap();
            assert!((native_alpha - alpha).abs() < 1e-6);
            assert!(
                (state[1].as_f64().unwrap() - native_alpha).abs() < 1e-6,
                "frame {frame}: {state}"
            );
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    #[ignore = "requires the local Mawaru 9 corpus and bundled judgment graphic"]
    fn mawaru9_opening_with_loaded_judgment() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let song_dir = workspace.join("lua-songs/mawaru9");
        let simfile = song_dir.join("mawaru9.sm");
        let graphic =
            workspace.join("deadsync/assets/graphics/judgements/Love 2x7 (doubleres).png");
        let context = Context {
            simfile: &simfile,
            song_dir: &song_dir,
            title: "MAWARU SIMULATOR 2016",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "TaroNuke",
            max_beat: 1.0,
            bpm: 115.0,
            bpm_segments: &[],
            beat_step: 0.125,
            max_events: 2_000_000,
            random_seed: 1,
        };
        let trace = evaluate_with_resources(
            &[Entry {
                path: song_dir.join("lua/default.lua"),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
            Some(reference_noteskin().expect("local reference noteskin")),
            Some(&graphic),
        )
        .expect("native Mawaru 9 opening");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        assert_eq!(trace["end_position"]["beat"], 1);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn initial_judgment_is_loaded_before_song_lua() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("initial-judgment.lua");
        let graphic = song_dir.join("Normal 2x6.png");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "initial judgment",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.5,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate_with_resources(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
            None,
            Some(&graphic),
        )
        .expect("native loaded judgment before song startup");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["judgment_reference"]["path"], "Normal 2x6.png");
        assert!(
            trace["judgment_reference"]["sha256"]
                .as_str()
                .unwrap()
                .len()
                == 64
        );
        assert!(
            !trace["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| { event["operation"] == "Sprite.Load" }),
            "theme setup must not invent a song load operation"
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn judgment_sheets_keep_unsampled_loads() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("judgment-sheet-changes.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "judgment sheet changes",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.5,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate_with_noteskin(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
            None,
        )
        .expect("native judgment hierarchy and resource changes");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let loads = trace["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["operation"] == "Sprite.Load")
            .collect::<Vec<_>>();
        assert_eq!(loads.len(), 2);
        for load in loads {
            assert_eq!(load["seconds"], 0);
            assert_eq!(load["args"][0], "Normal 2x6.png");
        }
        let updates = trace["callback_operation_tracks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|track| track["operation"] == "Sprite.Load")
            .collect::<Vec<_>>();
        assert_eq!(updates.len(), 2);
        for track in updates {
            let samples = track["samples"].as_array().unwrap();
            assert_eq!(samples.len(), 2);
            for (sample, (second, path)) in samples
                .iter()
                .zip([(0.05, "Fake 2x6.png"), (0.1, "Normal 2x6.png")])
            {
                assert!((sample[2].as_f64().unwrap() - second).abs() < 1e-6);
                assert_eq!(sample[3][0], path);
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn message_handlers_use_update_bound_actors() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("deferred-message.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "deferred message binding",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 3.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native deferred message trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let calls = trace["events"].as_array().unwrap();
        let alphas = calls
            .iter()
            .filter(|event| {
                event["command"] == "LateMessageCommand"
                    && event["operation"] == "Quad.diffusealpha"
            })
            .collect::<Vec<_>>();
        assert_eq!(alphas.len(), 2);
        for (call, expected) in alphas.iter().zip([0.4, 0.8]) {
            assert!((call["args"][0].as_f64().unwrap() - expected).abs() < 1e-6);
        }
        for (operation, expected) in [("Player.skewx", 0.25), ("Quad.rotationz", 15.0)] {
            let writes = calls
                .iter()
                .filter(|event| {
                    event["command"] == "LateMessageCommand" && event["operation"] == operation
                })
                .collect::<Vec<_>>();
            assert_eq!(writes.len(), 2, "{operation}");
            assert!(writes.iter().all(|event| event["args"][0] == expected));
        }
        assert!(
            !calls
                .iter()
                .any(|event| event["command"] == "UnsentMessageCommand"
                    || event["command"] == "BrokenMessageCommand")
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn player_setters_replace_pending_tween_destination() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("player-tween-destination.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "player tween destination",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 4.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native player destination trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let samples = trace["player_render_tracks"][0]["transform_samples"]
            .as_array()
            .unwrap();
        for (frame, zoom_x, zoom_y) in [
            (90, 1.375, 1.0),
            (120, 1.25, 1.0),
            (150, 0.5625, 0.625),
            (210, 0.25, 0.5),
        ] {
            let sample = samples
                .iter()
                .rev()
                .find(|sample| sample[0].as_u64().unwrap() <= frame)
                .unwrap();
            for (index, expected) in [(7, zoom_x), (8, zoom_y)] {
                assert!(
                    (sample[index].as_f64().unwrap() - expected).abs() < 1e-6,
                    "frame {frame}: {sample}"
                );
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn player_tweens_retain_immediate_starts() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("player-tween.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "player tween",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 2.1,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native player tween trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let samples = trace["runtime_actors"][0]["render_state_samples"]
            .as_array()
            .unwrap();
        for (frame, alpha) in [(60, 0.5), (75, 0.5), (90, 0.5)] {
            let sample = samples
                .iter()
                .rev()
                .find(|sample| sample[0].as_u64().unwrap() <= frame)
                .unwrap();
            assert!(
                (sample[1].as_f64().unwrap() - alpha).abs() < 1e-6,
                "frame {frame}: {sample}"
            );
        }
        let transforms = trace["player_render_tracks"][0]["transform_samples"]
            .as_array()
            .unwrap();
        for (frame, rotation_x, rotation_z, zoom_y, skew_x) in [
            (60, -30.0, 5.0, 1.18, 0.1),
            (75, -7.5, 1.25, 1.045, 0.025),
            (90, 0.0, 0.0, 1.0, 0.0),
            (106, 0.0, 0.0, 1.0, -0.1),
            (121, 0.0, 0.0, 1.0, 0.0),
        ] {
            let sample = transforms
                .iter()
                .rev()
                .find(|sample| sample[0].as_u64().unwrap() <= frame)
                .unwrap();
            for (index, value) in [(4, rotation_x), (5, rotation_z), (8, zoom_y), (10, skew_x)] {
                assert!(
                    (sample[index].as_f64().unwrap() - value).abs() < 1e-6,
                    "frame {frame}: {sample}"
                );
            }
        }

        let transforms = trace["player_render_tracks"][1]["transform_samples"]
            .as_array()
            .unwrap();
        for (frame, value) in [(60, 0.7), (75, 0.85), (90, 1.0)] {
            let sample = transforms
                .iter()
                .rev()
                .find(|sample| sample[0].as_u64().unwrap() <= frame)
                .unwrap();
            for index in [7, 8, 9] {
                assert!(
                    (sample[index].as_f64().unwrap() - value).abs() < 1e-6,
                    "frame {frame}: {sample}"
                );
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn diffuse_uses_native_colors_and_tween_alpha() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("headless fixtures");
        let entry = song_dir.join("diffuse.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 120.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "diffuse",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 2.0,
            bpm: 120.0,
            bpm_segments: &bpms,
            beat_step: 1.0,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native diffuse capture");
        assert_eq!(document["runtime_errors"], serde_json::json!([]));
        let definitions = document["actor_definitions"]
            .as_array()
            .expect("definitions");
        let tracks = document["projected_vertex_tracks"]
            .as_array()
            .expect("projected tracks");
        for (name, expected) in [
            ("numeric", vec![0.125]),
            ("table", vec![0.25]),
            ("tween", vec![0.125, 0.25, 0.375]),
            ("transparent", vec![0.0]),
            ("brightness", vec![0.5]),
            ("raw", vec![0.0]),
        ] {
            let definition = definitions
                .iter()
                .find(|value| value["name"] == name)
                .expect("color actor");
            let track = tracks
                .iter()
                .find(|value| value["definition_id"] == definition["id"])
                .expect("color projection");
            let samples = track["samples"].as_array().expect("samples");
            assert_eq!(samples.len(), expected.len(), "{name}");
            for (sample, alpha) in samples.iter().zip(expected) {
                assert!(
                    (sample[3].as_f64().expect("alpha") - alpha).abs() < 0.000_001,
                    "{name}: {sample}"
                );
                assert_eq!(sample[2], alpha > 0.0, "{name}");
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn render_alpha_keeps_nonfinite_kind() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("headless fixtures");
        let entry = song_dir.join("nonfinite-alpha.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 120.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "nonfinite alpha",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 120.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native nonfinite alpha capture");
        assert_eq!(document["runtime_errors"], serde_json::json!([]));
        let tracks = document["projected_vertex_tracks"]
            .as_array()
            .expect("projected tracks");
        assert_eq!(tracks.len(), 3);
        for (track, kind) in tracks.iter().zip(["infinity", "-infinity", "nan"]) {
            let actor = document["runtime_actors"]
                .as_array()
                .expect("runtime actors")
                .iter()
                .find(|actor| actor["id"] == track["actor"])
                .expect("native actor for projected track");
            let alpha = serde_json::json!({"type": "number", "value": kind});
            assert_eq!(actor["final_render_state"]["alpha"], alpha);
            assert_eq!(
                actor["render_state_samples"],
                serde_json::json!([[0, alpha, true]])
            );
            assert_eq!(
                track["samples"][0][3],
                serde_json::json!({"type": "number", "value": kind})
            );
            assert_eq!(
                track["samples"][0][9][3],
                serde_json::json!({"type": "number", "value": kind}),
                "draw RGBA must retain its nonfinite component"
            );
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn boolean_writes_keep_unsampled_frames() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("headless fixtures");
        let entry = song_dir.join("boolean-frames.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 120.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "boolean frame capture",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 120.0,
            bpm_segments: &bpms,
            beat_step: 0.5,
            max_events: 800,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native boolean frame capture");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let frames = trace["update_frames"].as_array().expect("reference frames");
        let events = trace["events"].as_array().expect("setter events");
        let writes = events
            .iter()
            .filter(|event| event["operation"] == "PlayerOptions.StealthPastReceptors")
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 9);
        assert_eq!(writes.len(), frames.len(), "retain every boolean write");
        for (index, (write, frame)) in writes.iter().zip(frames).enumerate() {
            assert_eq!(write["beat"], frame[0]);
            assert_eq!(write["seconds"], frame[1]);
            assert_eq!(
                write["detail"]["boolean_option"],
                serde_json::json!({
                    "previous": index > 0, "current": true, "chained": false,
                })
            );
        }
        assert_eq!(
            events
                .iter()
                .filter(|event| event["operation"] == "PlayerOptions.Dark")
                .count(),
            2,
            "numeric setters retain their existing cadence"
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn noteskin_writes_use_native_strings_on_every_frame() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize().expect("headless fixtures");
        let entry = song_dir.join("noteskin-options.lua");
        let bpms = [BpmSegment { beat: 0.0, bpm: 120.0 }];
        let context = Context {
            simfile: &entry, song_dir: &song_dir, title: "noteskin option capture",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single",
            description: "", max_beat: 0.25, bpm: 120.0, bpm_segments: &bpms,
            beat_step: 0.5, max_events: 1500, random_seed: 1,
        };
        let trace = evaluate(&[Entry {
            path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
        }], &context).expect("native noteskin strings");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let frames = trace["update_frames"].as_array().expect("native frames");
        assert_eq!(frames.len(), 9);
        let writes = trace["events"].as_array().expect("native events").iter()
            .filter(|event| event["detail"]["noteskin_option"].is_object())
            .collect::<Vec<_>>();
        assert_eq!(writes.len(), 2 + 4 * frames.len(), "retain startup and unsampled calls");
        assert_eq!(writes[0]["detail"]["noteskin_option"]["previous"], "cyber");
        assert_eq!(writes[0]["detail"]["noteskin_option"]["current"], "default");
        assert_eq!(writes[1]["detail"]["noteskin_option"]["current"], "cel");
        for (index, frame) in frames.iter().enumerate() {
            let writes = &writes[2 + 4 * index..6 + 4 * index];
            for write in writes {
                assert_eq!(write["beat"], frame[0]);
                assert_eq!(write["seconds"], frame[1]);
            }
            assert_eq!(writes[0]["detail"]["noteskin_option"]["previous"],
                if index == 0 { "cel" } else { "CYBER" });
            assert_eq!(writes[0]["detail"]["noteskin_option"]["parts"],
                serde_json::json!([{ "part": "50% default", "target": "default" }]));
            assert_eq!(writes[1]["detail"]["noteskin_option"]["current"], "cyber");
            assert_eq!(writes[2]["detail"]["noteskin_option"]["current"], "CYBER");
            assert_eq!(writes[3]["detail"]["noteskin_option"]["current"], "CYBER");
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn modifier_queries_use_native_parsers_and_option_values() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("modifier-query.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 120.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "modifier queries",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 120.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 800,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let mirror_writes = trace["events"]
            .as_array()
            .expect("captured events")
            .iter()
            .filter(|event| event["operation"] == "PlayerOptions.Mirror")
            .collect::<Vec<_>>();
        assert_eq!(
            mirror_writes.len(),
            5,
            "boolean queries must not emit writes"
        );
        assert!(
            mirror_writes
                .iter()
                .all(|event| event["args"][0].is_boolean())
        );
        assert!(mirror_writes.iter().all(|event| {
            let state = &event["detail"]["boolean_option"];
            state["previous"].is_boolean()
                && state["current"] == event["args"][0]
                && state["chained"].is_boolean()
        }));
        assert!(
            trace["events"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| { event["operation"] == "Quad.x" && event["args"][0] == 42 })
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn avi_geometry_uses_video_stream_format() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("fixtures");
        let entry = song_dir.join("avi-metadata.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 120.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "AVI metadata",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 120.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native AVI geometry");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let tracks = trace["projected_vertex_tracks"]
            .as_array()
            .expect("video tracks");
        assert_eq!(tracks.len(), 2, "capture both bitmap orientations");
        for (track, center_x) in tracks.iter().zip([100.0, 200.0]) {
            assert_eq!(track["texture_size"], serde_json::json!([2, 2]));
            let corners = track["samples"][0][6].as_array().expect("video corners");
            for (corner, [x, y]) in corners.iter().zip([
                [center_x - 2.0, 98.0],
                [center_x + 2.0, 98.0],
                [center_x + 2.0, 102.0],
                [center_x - 2.0, 102.0],
            ]) {
                assert!((corner[0].as_f64().expect("x") - x).abs() < 0.0001);
                assert!((corner[1].as_f64().expect("y") - y).abs() < 0.0001);
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn jpeg_geometry_uses_native_texture_metadata() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("fixtures");
        let entry = song_dir.join("jpeg-metadata.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 120.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "JPEG metadata",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 120.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native JPEG geometry");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let tracks = trace["projected_vertex_tracks"]
            .as_array()
            .expect("image tracks");
        assert_eq!(tracks.len(), 2, "capture baseline and progressive JPEG");
        for (track, center_x, width, height) in [
            (&tracks[0], 100.0, 64.0, 48.0),
            (&tracks[1], 300.0, 16.0, 16.0),
        ] {
            for (value, expected) in track["texture_size"]
                .as_array()
                .expect("source dimensions")
                .iter()
                .zip([width, height])
            {
                assert_eq!(value.as_f64().expect("dimension"), expected);
            }
            let corners = track["samples"][0][6].as_array().expect("image corners");
            for (corner, [x, y]) in corners.iter().zip([
                [center_x - width, 100.0 - height],
                [center_x + width, 100.0 - height],
                [center_x + width, 100.0 + height],
                [center_x - width, 100.0 + height],
            ]) {
                assert!((corner[0].as_f64().expect("x") - x).abs() < 0.0001);
                assert!((corner[1].as_f64().expect("y") - y).abs() < 0.0001);
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn legacy_arrow_call_cancels_native_rotation_alias() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("fixtures");
        let entry = song_dir.join("legacy-arrow.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 120.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "legacy arrow",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 120.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("legacy rotation trace");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let rotations = trace["events"]
            .as_array()
            .expect("events")
            .iter()
            .filter(|event| event["operation"] == "Quad.rotationx")
            .collect::<Vec<_>>();
        assert_eq!(rotations.len(), 2);
        for event in rotations {
            assert!(
                event["args"][0].as_f64().expect("rotation").abs() < 0.00001,
                "{event}"
            );
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn native_noteskin_templates_and_metrics_are_captured() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = workspace
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("fixture directory");
        let entry = song_dir.join("noteskin.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "native noteskin",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.5,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 2000,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native noteskin trace");
        assert_eq!(document["runtime_errors"], serde_json::json!([]));
        assert!(
            document["actor_definitions"]
                .as_array()
                .expect("actors")
                .len()
                > 10
        );
        assert!(
            document["events"]
                .as_array()
                .expect("events")
                .iter()
                .any(|event| {
                    event["command"] == "W1Command"
                        && event["operation"] == "Sprite.diffusealpha"
                        && event["args"][0] == 1.2
                }),
            "the native metric command must light the W1 sprite"
        );
        assert!(
            document["loaded_lua_files"]
                .as_array()
                .expect("dependencies")
                .iter()
                .any(|path| path == "noteskin:/dance/cyber/Fallback Explosion.lua")
        );
        let files = document["noteskin_reference"]["files"]
            .as_array()
            .expect("hashes");
        assert!(
            files
                .iter()
                .any(|file| file["path"] == "dance/cyber/Fallback Explosion.lua"
                    && file["sha256"].as_str().is_some_and(|hash| hash.len() == 64))
        );
        let missing = evaluate_with_noteskin(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
            None,
        );
        assert!(
            matches!(missing, Err(Error::Native(message)) if message.contains("native noteskin resources unavailable"))
        );
    }

    #[test]
    fn rejects_truncated_response() {
        let mut input = Reader::new(MAGIC);
        assert_eq!(input.take(MAGIC.len()).unwrap(), MAGIC);
        assert!(matches!(input.u32(), Err(Error::Wire(_))));
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn loads_relative_helper_files_in_the_embedded_host() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("default.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 120.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "headless helper",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 120.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(document["actor_definitions"][0]["name"], "loaded");
        assert_eq!(
            document["loaded_lua_files"],
            serde_json::json!(["song:/default.lua", "song:/helper.lua"])
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn captures_single_and_double_style_contexts() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("headless fixture directory");
        let entry = song_dir.join("style.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 120.0,
        }];
        for (steps_type, style, enabled) in [
            ("dance-single", "single", [true, true]),
            ("dance-double", "double", [true, false]),
        ] {
            let context = Context {
                simfile: &entry,
                song_dir: &song_dir,
                title: "headless style",
                difficulty: "Difficulty_Challenge",
                steps_type,
                description: "",
                max_beat: 0.25,
                bpm: 120.0,
                bpm_segments: &bpms,
                beat_step: 0.25,
                max_events: 100,
                random_seed: 1,
            };
            let document = evaluate(
                &[Entry {
                    path: entry.clone(),
                    layer: "foreground",
                    index: 0,
                    start_beat: 0.0,
                }],
                &context,
            )
            .expect("capture style fixture");
            assert_eq!(document["runtime_errors"], serde_json::json!([]));
            assert_eq!(document["style"], style);
            assert_eq!(document["enabled_players"], serde_json::json!(enabled));
            assert_eq!(document["actor_definitions"][0]["name"], "style-verified");
            let actors = document["external_actors"].as_array().expect("external actors");
            assert_eq!(actors.iter().any(|actor| actor["path"] == "ScreenGameplay/PlayerP2"),
                enabled[1], "disabled players must not enter the screen actor tree");
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn samples_current_relative_tween_state_in_update_callbacks() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("tween.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "headless tween",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        let samples = document["callback_operation_tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|track| track["operation"] == "Quad.y")
            .unwrap()["samples"]
            .as_array()
            .unwrap();
        // Native float subtraction retains a small remainder at the midpoint.
        for (beat, expected) in [
            (0.0, 0.0),
            (0.5, 49.999_954_223_632_81),
            (1.0, 99.999_954_223_632_81),
        ] {
            let sample = samples
                .iter()
                .find(|sample| {
                    sample[1]
                        .as_f64()
                        .is_some_and(|value| (value - beat).abs() < 1e-9)
                })
                .unwrap_or_else(|| panic!("missing sample at beat {beat}"));
            let value = sample[3][0].as_f64().expect("numeric tween sample");
            assert!(
                (value - expected).abs() < 1e-9,
                "unexpected sample at beat {beat}: {value}"
            );
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn starts_scheduled_tweens_at_their_exact_beat() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("scheduled-tween.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "headless scheduled tween",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.5,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        let samples = document["callback_operation_tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|track| track["operation"] == "Quad.y")
            .unwrap()["samples"]
            .as_array()
            .unwrap();
        let quarter = samples
            .iter()
            .find(|sample| sample[1] == 0.25)
            .expect("missing quarter-beat sample");
        let value = quarter[3][0].as_f64().expect("numeric tween sample");
        assert!(
            (value - 9.999_990_463_256_836).abs() < 1e-9,
            "unexpected tween sample: {value}"
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn updates_parent_commands_before_child_tweens() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("parent-update.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "parent update order",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.3,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 16.0 / 60.0,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        let samples = document["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["operation"] == "Quad.y")
            .collect::<Vec<_>>();
        let update = samples
            .iter()
            .find(|event| event["beat"].as_f64().is_some_and(|beat| beat > 0.0))
            .expect("missing parent update");
        let beat = update["beat"].as_f64().expect("numeric update beat");
        let value = update["args"][0].as_f64().expect("numeric tween sample");
        assert!(
            (beat - 16.0 / 60.0).abs() < 1e-6,
            "unexpected update beat: {beat}"
        );
        let expected = 24.999_977_111_816_406;
        assert!(
            (value - expected).abs() < 1e-6,
            "unexpected parent-before-child sample: {value}"
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn texture_coordinates_keep_native_allocation_space() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
        let entry = song_dir.join("texture-coordinates.lua");
        let context = Context {
            simfile: &entry, song_dir: &song_dir, title: "Texture coordinates",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single",
            description: "", max_beat: 0.2, bpm: 60.0, bpm_segments: &[],
            beat_step: 0.025, max_events: 1000, random_seed: 1,
        };
        let trace = evaluate(&[Entry {
            path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
        }], &context).expect("native texture coordinate scene");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let completed = trace["events"].as_array().expect("native events").iter()
            .filter(|event| event["operation"] == "Sprite.x" && event["args"] == serde_json::json!([140]))
            .count();
        assert_eq!(completed, 1, "coordinate assertions completed");
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn captures_image_texture_aliases() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("texture-alias.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Texture aliases",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.2,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.025,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let tracks = trace["projected_vertex_tracks"].as_array().unwrap();
        assert_eq!(tracks.len(), 3, "source and both texture aliases");
        for (id, center, half_height) in [("def-0003", 100, 32), ("def-0004", 200, 16)] {
            let track = tracks
                .iter()
                .find(|track| track["definition_id"] == id)
                .unwrap();
            assert_eq!(track["texture_size"], serde_json::json!([64, 32]));
            assert!(track["texture"].as_str().unwrap().ends_with("fit-rect.png"));
            let vertices = track["samples"].as_array().unwrap().last().unwrap()[6]
                .as_array()
                .unwrap();
            assert_eq!(
                vertices[0],
                serde_json::json!([center - 32, 100 - half_height])
            );
            assert_eq!(
                vertices[2],
                serde_json::json!([center + 32, 100 + half_height])
            );
            let bindings = track["texture_samples"].as_array().unwrap();
            assert_eq!(bindings.len(), if id == "def-0003" { 2 } else { 1 });
            let binding = bindings.last().unwrap();
            assert_eq!(binding[4], half_height * 2);
            assert!(binding[2].as_str().unwrap().ends_with(if id == "def-0003" {
                "Normal 2x6.png"
            } else {
                "fit-rect.png"
            }));
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn captures_current_crop_values() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("crop-samples.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Crop samples",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.2,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.025,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        assert_eq!(trace["capabilities"]["sprite_crop_samples"], true);
        assert_eq!(trace["capabilities"]["sprite_shadow_samples"], true);
        let track = &trace["projected_vertex_tracks"][0];
        assert_eq!(track["sample_layout"][11], "crop");
        assert_eq!(track["sample_layout"][12], "shadow");
        let samples = track["samples"].as_array().unwrap();
        assert!(
            samples.len() >= 2,
            "crop changes must invalidate the projection signature"
        );
        let first = samples.first().unwrap()[11].as_array().unwrap();
        let last = samples.last().unwrap()[11].as_array().unwrap();
        for (actual, expected) in first.iter().zip([0.25, 0.1, 0.125, 0.2]) {
            assert!((actual.as_f64().unwrap() - expected).abs() < 1e-7);
        }
        assert_eq!(last[2], 1);
        assert_eq!(samples.first().unwrap()[12][0], 2);
        assert_eq!(samples.first().unwrap()[12][1], 3);
        let immediate = samples
            .iter()
            .find(|sample| sample[12][0] == 7)
            .expect("current shadow changes");
        assert_eq!(immediate[12][1], -2);
        assert!(
            (immediate[11][2].as_f64().unwrap() - 0.125).abs() < 1e-7,
            "shadow changes immediately while the queued crop remains pending"
        );
        for (actual, expected) in immediate[12].as_array().unwrap()[2..]
            .iter()
            .zip([0.2, 0.3, 0.4, 0.5])
        {
            assert!((actual.as_f64().unwrap() - expected).abs() < 1e-7);
        }
        assert!(
            samples.last().unwrap()[1].as_f64().unwrap() > 0.1,
            "queued crop applies after the parent callback's frame"
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn captures_late_aft_texture_geometry() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("late-texture.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Late Texture",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.2,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.05,
            max_events: 10000,
            random_seed: 1,
        };
        let mut trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        crate::song_lua_semantics::compact_fixture(&mut trace).unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let tracks = trace["projected_vertex_tracks"].as_array().unwrap();
        assert_eq!(tracks.len(), 2);
        for track in tracks {
            assert!(track["texture"].as_str().unwrap().starts_with("aft:"));
            assert_eq!(track["texture_size"], serde_json::json!([96, 48]));
            assert!(
                track["samples"].as_array().unwrap().first().unwrap()[0]
                    .as_f64()
                    .unwrap()
                    >= 0.2
            );
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn rejected_parts_keep_native_float_state() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("native Lua fixture directory");
        let entry = song_dir.join("rejected-option-parts.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Rejected modifier parts",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(&[Entry {
            path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
        }], &context).expect("native rejected-part reference");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let parts = trace["events"].as_array().expect("native events").iter()
            .find_map(|event| event["detail"]["rejected_parts"].as_array())
            .expect("retained parser rejection evidence");
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["key"], "bumpperiod");
        assert_eq!(parts[1]["key"], "completely_unknown");
        for part in parts {
            assert_eq!(part["accepted"], false);
            assert_eq!(part["unchanged"], true);
            let values = part["values"].as_array().expect("native field snapshot");
            assert!(values.len() > 100);
            for (key, amount, speed) in [("bumpyperiod", -0.66, 7.0), ("drunk", 0.25, 3.0)] {
                let value = values.iter().find(|value| value[0] == key)
                    .expect("native option amount and approach speed");
                assert!((value[1].as_f64().expect("native amount") - amount).abs() < 0.000001);
                assert_eq!(value[2], speed);
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn indexed_noops_keep_native_field_snapshots() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("indexed-option-noops.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Indexed no-ops",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("native indexed no-op reference");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let event = trace["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["detail"]["indexed_noops"].is_array())
            .expect("retained no-op event");
        let noops = event["detail"]["indexed_noops"].as_array().unwrap();
        assert_eq!(noops.len(), 4);
        for noop in noops {
            assert_eq!(noop["unchanged"], true);
            let values = noop["values"].as_array().unwrap();
            assert_eq!(
                values.len(),
                if noop["key"] == "confusionoffset0" {
                    5
                } else {
                    4
                }
            );
            for value in values {
                let expected = match value[0].as_str().unwrap() {
                    "confusionoffset" => 0.7,
                    "movex1" => 0.25,
                    "movey4" => -0.5,
                    _ => 0.0,
                };
                assert!((value[1].as_f64().unwrap() - expected).abs() < 0.000001);
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn named_aft_sprites_keep_geometry() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("named-aft.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Named AFT",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("named render-target reference");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let tracks = trace["projected_vertex_tracks"].as_array().unwrap();
        assert_eq!(
            tracks.len(),
            3,
            "property, SetTexture and Load must all draw"
        );
        for (track, (x, scale)) in tracks
            .iter()
            .zip([(100.0, 1.0), (200.0, 1.0), (300.0, 2.0)])
        {
            assert_eq!(track["texture"], "aft:def-0002");
            assert_eq!(track["texture_size"], serde_json::json!([64, 32]));
            let vertices = track["samples"][0][6].as_array().unwrap();
            for (vertex, [dx, dy]) in
                vertices
                    .iter()
                    .zip([[-32.0, -16.0], [32.0, -16.0], [32.0, 16.0], [-32.0, 16.0]])
            {
                assert!((vertex[0].as_f64().unwrap() - (x + dx * scale)).abs() < 0.0001);
                assert!((vertex[1].as_f64().unwrap() - (100.0 + dy * scale)).abs() < 0.0001);
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn captures_due_action_when_queued_update_wakes_late() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("queued-action.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "queued action capture",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.2,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 1.0,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        let visible = document["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| {
                event["command"] == "UpdateCommand"
                    && event["operation"] == "Quad.visible"
                    && event["args"][0] == true
            })
            .expect("queued action visibility call was not captured");
        let beat = visible["beat"].as_f64().expect("numeric action beat");
        assert!(
            beat >= 0.1 && beat < 0.12,
            "queued action ran at unexpected beat {beat}"
        );
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn playcommand_propagates_through_actor_frame_descendants() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("playcommand-propagation.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "playcommand propagation",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        let child_runs = document["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| {
                event["command"] == "ChildCommand" && event["operation"] == "command.begin"
            })
            .map(|event| event["actor"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(child_runs, ["def-0001", "def-0002", "def-0003"]);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn coerces_numeric_visibility_and_records_hidden_geometry() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("numeric-visible.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "numeric visibility",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        let definitions = document["actor_definitions"].as_array().unwrap();
        let id_for = |name: &str| {
            definitions
                .iter()
                .find(|definition| definition["name"] == name)
                .and_then(|definition| definition["id"].as_str())
                .unwrap()
        };
        let tracks = document["projected_vertex_tracks"].as_array().unwrap();
        let hidden = tracks
            .iter()
            .find(|track| track["definition_id"] == id_for("hidden"))
            .expect("hidden actor must retain an explicit geometry track");
        let shown = tracks
            .iter()
            .find(|track| track["definition_id"] == id_for("shown"))
            .expect("shown actor geometry track");
        assert_eq!(hidden["samples"][0][2], false);
        assert!(hidden["samples"][0][6].as_array().unwrap().is_empty());
        assert_eq!(shown["samples"][0][2], true);
        assert_eq!(shown["samples"][0][6].as_array().unwrap().len(), 4);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn aft_boundaries_match_native_drawing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("aft-boundary.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "AFT Boundary",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 1000,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(document["runtime_errors"], serde_json::json!([]));
        assert_eq!(document["dropped_events"], 0);
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/aft-boundary.json"))
                .unwrap();
        assert_eq!(
            native,
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/aft-boundary.json"))
                .unwrap()
        );
        let definitions = document["actor_definitions"].as_array().unwrap();
        let tracks = document["projected_vertex_tracks"].as_array().unwrap();
        let actors = native["samples"][0]["actors"].as_array().unwrap();
        let mut checked = 0;
        for actor in actors.iter().filter(|actor| actor["kind"] == "sprite") {
            let definition = definitions
                .iter()
                .find(|def| def["name"] == actor["name"])
                .unwrap();
            let track = tracks
                .iter()
                .find(|track| track["definition_id"] == definition["id"])
                .unwrap();
            let sample = &track["samples"][0];
            assert_eq!(sample[2], actor["visible"], "{} visibility", actor["name"]);
            if actor["visible"] == false {
                continue;
            }
            let vertices = actor["draws"][0]["vertices"].as_array().unwrap();
            // Sprite draws TL, BL, BR, TR; semantic tracks record TL, TR, BR, BL.
            for (index, actual) in sample[6].as_array().unwrap().iter().enumerate() {
                let vertex = &vertices[[0, 3, 2, 1][index]];
                for axis in 0..2 {
                    let expected = vertex["screen"][axis].as_f64().unwrap();
                    assert!(
                        (expected - actual[axis].as_f64().unwrap()).abs() < 0.0002,
                        "{} screen axis {axis}: {expected} vs {actual}",
                        actor["name"]
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 48);
        for name in [
            "ScreenBackground",
            "SmallBackground",
            "AfterSmall",
            "AfterTarget",
            "VisibilityProbe",
        ] {
            assert_eq!(
                actors.iter().find(|actor| actor["name"] == name).unwrap()["visible"],
                true
            );
        }
        let combo = document["runtime_actors"]
            .as_array()
            .unwrap()
            .iter()
            .find(|actor| actor["name"] == "ComboCopy")
            .unwrap();
        assert_eq!(combo["final_render_state"]["visible"], true);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn song_position_keeps_strict_beat_boundaries() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("beat-boundary.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 200.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Beat Boundary",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 62.3,
            bpm: 200.0,
            bpm_segments: &bpms,
            beat_step: 0.03125,
            max_events: 10000,
            random_seed: 1,
        };
        let mut trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        crate::song_lua_semantics::compact_fixture(&mut trace).unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        for (name, expected) in [
            ("Strict", 62.0 + 1.0 / 18.0),
            ("Inclusive", 62.0),
            ("Position", 62.0 + 1.0 / 18.0),
        ] {
            let definition = trace["actor_definitions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|actor| actor["name"] == name)
                .unwrap();
            let track = trace["operation_tracks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|track| track["actor"] == definition["id"] && track["operation"] == "Quad.x")
                .expect("quad x track");
            let sample = &track["samples"][0];
            assert!(
                (sample[1].as_f64().unwrap() - expected).abs() <= 0.00001,
                "{name}: {sample}"
            );
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn bpm_fade_matches_native_alpha() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("bpm-fade.lua");
        let bpms = [
            BpmSegment {
                beat: 0.0,
                bpm: 333.0,
            },
            BpmSegment {
                beat: 5.0,
                bpm: 666.0,
            },
        ];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "BPM Fade",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 11.0,
            bpm: 333.0,
            bpm_segments: &bpms,
            beat_step: 0.03125,
            max_events: 10000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/bpm-fade.json"))
                .unwrap();
        let definition = trace["actor_definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["name"] == "Fade")
            .unwrap();
        let track = trace["projected_vertex_tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|track| track["definition_id"] == definition["id"])
            .unwrap();
        let mut checks = 0;
        for sample in native["samples"].as_array().unwrap() {
            let second = sample["time"].as_f64().unwrap();
            let projected = track["samples"]
                .as_array()
                .unwrap()
                .iter()
                .rev()
                // Projected tracks omit unchanged frames; hold the last sample.
                .find(|value| value[1].as_f64().unwrap() <= second + 1e-6)
                .unwrap();
            let actor = sample["actors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|actor| actor["name"] == "Fade")
                .unwrap();
            let expected = actor["current"]["diffuse"][0][3].as_f64().unwrap();
            let actual = projected[3].as_f64().unwrap();
            assert!(
                (expected - actual).abs() <= 0.0001,
                "fade alpha at {second}: {actual} vs native {expected}"
            );
            checks += 1;
        }
        assert_eq!(checks, 9);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn aliases_match_native_alignment() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("layout-alignment.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Layout Alignment",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.03125,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/layout-alignment.json"))
                .unwrap();
        let mut checks = 0;
        for actor in native["samples"][0]["actors"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|actor| actor["name"] != "root")
        {
            let definition = trace["actor_definitions"]
                .as_array()
                .unwrap()
                .iter()
                .find(|def| def["name"] == actor["name"])
                .unwrap();
            let track = trace["projected_vertex_tracks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|track| track["definition_id"] == definition["id"])
                .unwrap();
            for corner in 0..4 {
                for axis in 0..2 {
                    let expected = actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["screen"]
                        [axis]
                        .as_f64()
                        .unwrap();
                    let actual = track["samples"][0][6][corner][axis].as_f64().unwrap();
                    assert!(
                        (expected - actual).abs() <= 0.002,
                        "{} corner {corner} axis {axis}: {expected} != {actual}",
                        actor["name"]
                    );
                    checks += 1;
                }
            }
        }
        assert_eq!(checks, 48);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn zoom_axis_fit_matches_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("zoom-axis-fit.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Zoom axis fit",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 1.0 / 60.0,
            max_events: 5000,
            random_seed: 1,
        };
        let fixture = root.join("fixtures/actors/zoom-axis-fit.json");
        crate::actor_conformance::evaluate(&fixture).unwrap();
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native = crate::actor_conformance::evaluate(&fixture).unwrap();
        let mut checks = 0;
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            for actor in sample["actors"].as_array().unwrap().iter().skip(1) {
                let definition = trace["actor_definitions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|def| def["name"] == actor["name"])
                    .unwrap();
                let track = trace["projected_vertex_tracks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|track| track["actor"] == definition["id"])
                    .unwrap();
                let row = track["samples"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|row| row[1].as_f64().unwrap() <= time + 0.000001)
                    .unwrap();
                for corner in 0..4 {
                    for axis in 0..2 {
                        let expected =
                            actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["screen"][axis]
                                .as_f64()
                                .unwrap();
                        let actual = row[6][corner][axis].as_f64().unwrap();
                        assert!(
                            (expected - actual).abs() <= 0.002,
                            "{} at {time} corner {corner} axis {axis}: {expected} != {actual}",
                            actor["name"]
                        );
                        checks += 1;
                    }
                }
            }
        }
        assert_eq!(checks, 120);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn fallback_tweens_match_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("fallback-tweens.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Fallback Tweens",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.5,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 1.0 / 60.0,
            max_events: 5000,
            random_seed: 1,
        };
        // Prime the persistent native registry before the semantic session replaces
        // its shared enum references. The second evaluation must remain identical.
        crate::actor_conformance::evaluate(&root.join("fixtures/actors/fallback-tweens.json"))
            .unwrap();
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/fallback-tweens.json"))
                .unwrap();
        let definitions = trace["actor_definitions"].as_array().unwrap();
        let tracks = trace["projected_vertex_tracks"].as_array().unwrap();
        let mut checks = 0;
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            for actor in sample["actors"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a["kind"] == "sprite")
            {
                let definition = definitions
                    .iter()
                    .find(|a| a["name"] == actor["name"])
                    .unwrap();
                let track = tracks
                    .iter()
                    .find(|t| t["definition_id"] == definition["id"])
                    .unwrap();
                let actual = track["samples"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|row| row[1].as_f64().unwrap() <= time + 0.000001)
                    .unwrap();
                assert_eq!(actual[2], actor["visible"], "{} at {time}", actor["name"]);
                checks += 1;
                if actor["visible"] != true {
                    continue;
                }
                for corner in 0..4 {
                    for axis in 0..2 {
                        let expected =
                            actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["screen"][axis]
                                .as_f64()
                                .unwrap();
                        let actual = actual[6][corner][axis].as_f64().unwrap();
                        assert!(
                            (expected - actual).abs() <= 0.002,
                            "{} at {time}: {expected} vs {actual}",
                            actor["name"]
                        );
                        checks += 1;
                    }
                }
            }
        }
        assert_eq!(checks, 184);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn spin_tweens_match_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("spin-tween.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Spin Tweens",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 2.0,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 1.0 / 60.0,
            max_events: 5000,
            random_seed: 1,
        };
        // Prime the persistent native registry before the semantic session replaces
        // its shared enum references. The second evaluation must remain identical.
        crate::actor_conformance::evaluate(&root.join("fixtures/actors/spin-tween.json")).unwrap();
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/spin-tween.json"))
                .unwrap();
        let definitions = trace["actor_definitions"].as_array().unwrap();
        let tracks = trace["projected_vertex_tracks"].as_array().unwrap();
        let mut checks = 0;
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            for actor in sample["actors"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|a| a["kind"] == "sprite")
            {
                let definition = definitions
                    .iter()
                    .find(|a| a["name"] == actor["name"])
                    .unwrap();
                let track = tracks
                    .iter()
                    .find(|t| t["definition_id"] == definition["id"])
                    .unwrap();
                let actual = track["samples"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|row| row[1].as_f64().unwrap() <= time + 0.000001)
                    .unwrap();
                assert_eq!(actual[2], actor["visible"], "{} at {time}", actor["name"]);
                checks += 1;
                if actor["visible"] != true {
                    continue;
                }
                for corner in 0..4 {
                    for axis in 0..2 {
                        let expected =
                            actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["screen"][axis]
                                .as_f64()
                                .unwrap();
                        let actual = actual[6][corner][axis].as_f64().unwrap();
                        assert!(
                            (expected - actual).abs() <= 0.002,
                            "{} at {time}: {expected} vs {actual}",
                            actor["name"]
                        );
                        checks += 1;
                    }
                }
            }
        }
        assert_eq!(checks, 2178);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn callback_precedes_later_siblings() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("callback-phase.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Callback Phase",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.5,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 1.0 / 60.0,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let tracks = trace["projected_vertex_tracks"].as_array().unwrap();
        for (id, expected) in [("def-0002", 100.0), ("def-0004", 100.0 + 100.0 / 12.0)] {
            let track = tracks.iter().find(|t| t["definition_id"] == id).unwrap();
            let sample = track["samples"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|row| row[1].as_f64().unwrap() <= 0.2 + 1e-7)
                .unwrap();
            let x = sample[6]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v[0].as_f64().unwrap())
                .sum::<f64>()
                / 4.0;
            // ActorFrame::UpdateInternal runs this callback before the later
            // sibling advances, so only After consumes the first tween delta.
            assert!((x - expected).abs() < 0.0001, "{id}: {x} vs {expected}");
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn base_zoom_matches_native_drawing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("waltz-runtime.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Waltz Runtime",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 3.0,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.03125,
            max_events: 10000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/waltz-base-zoom.json"))
                .unwrap();
        let definition = trace["actor_definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|def| def["name"] == "Sprite")
            .unwrap();
        let track = trace["projected_vertex_tracks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|track| track["definition_id"] == definition["id"])
            .unwrap();
        let mut checks = 0;
        for sample in native["samples"].as_array().unwrap() {
            let second = sample["time"].as_f64().unwrap();
            let projected = track["samples"]
                .as_array()
                .unwrap()
                .iter()
                .min_by(|a, b| {
                    (a[1].as_f64().unwrap() - second)
                        .abs()
                        .total_cmp(&(b[1].as_f64().unwrap() - second).abs())
                })
                .unwrap();
            let actor = sample["actors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|actor| actor["name"] == "Sprite")
                .unwrap();
            for corner in 0..4 {
                for axis in 0..2 {
                    let expected = actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["screen"]
                        [axis]
                        .as_f64()
                        .unwrap();
                    let actual = projected[6][corner][axis].as_f64().unwrap();
                    assert!(
                        (expected - actual).abs() <= 0.002,
                        "base zoom at {second}: {expected} vs {actual}"
                    );
                    checks += 1;
                }
            }
        }
        assert_eq!(checks, 56);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn wrappers_match_native_drawing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("wrapper-transform.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Wrapper transform",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 4.0,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 10000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native = crate::actor_conformance::evaluate(
            &root.join("fixtures/actors/wrapper-transform.json"),
        )
        .unwrap();
        let definitions = trace["actor_definitions"].as_array().unwrap();
        let tracks = trace["projected_vertex_tracks"].as_array().unwrap();
        let mut checks = 0;
        for sample in native["samples"].as_array().unwrap() {
            let second = sample["time"].as_f64().unwrap();
            for actor in sample["actors"].as_array().unwrap().iter().skip(1) {
                let definition = definitions
                    .iter()
                    .find(|d| d["name"] == actor["name"])
                    .unwrap();
                let track = tracks
                    .iter()
                    .find(|t| t["definition_id"] == definition["id"])
                    .unwrap();
                let projected = track["samples"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|s| s[1].as_f64().unwrap() <= second + 0.00001)
                    .last()
                    .unwrap();
                let expected_alpha = actor["draws"][0]["vertices"][0]["color"][3]
                    .as_f64()
                    .unwrap()
                    / 255.0;
                assert!((projected[3].as_f64().unwrap() - expected_alpha).abs() <= 1.0 / 255.0);
                for corner in 0..4 {
                    for axis in 0..3 {
                        let expected = actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["world"]
                            [axis]
                            .as_f64()
                            .unwrap();
                        let actual = projected[4][corner][axis].as_f64().unwrap();
                        assert!(
                            (expected - actual).abs() <= 0.002,
                            "{} at {second}, corner {corner}/{axis}: {expected} vs {actual}",
                            actor["name"]
                        );
                        checks += 1;
                    }
                }
            }
        }
        assert_eq!(checks, 216);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn perspective_float_matches_native_drawing() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("perspective-float.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Perspective Float",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 83.7,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 83.7,
            max_events: 1000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let path = root.join("fixtures/actors/perspective-float.json");
        let native = crate::actor_conformance::evaluate(&path).unwrap();
        assert_eq!(native, crate::actor_conformance::evaluate(&path).unwrap());
        let mut checks = 0;
        for sample in native["samples"].as_array().unwrap() {
            for actor in sample["actors"].as_array().unwrap().iter().filter(|actor| {
                actor["draws"]
                    .as_array()
                    .is_some_and(|draws| !draws.is_empty())
            }) {
                let definition = trace["actor_definitions"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|def| def["name"] == actor["name"])
                    .unwrap();
                let track = trace["projected_vertex_tracks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|track| track["definition_id"] == definition["id"])
                    .unwrap();
                let second = sample["time"].as_f64().unwrap();
                let projected = track["samples"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .min_by(|a, b| {
                        (a[1].as_f64().unwrap() - second)
                            .abs()
                            .total_cmp(&(b[1].as_f64().unwrap() - second).abs())
                    })
                    .unwrap();
                for (corner, actual) in projected[6].as_array().unwrap().iter().enumerate() {
                    let vertex = &actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]];
                    for axis in 0..2 {
                        let expected = vertex["screen"][axis].as_f64().unwrap();
                        assert!(
                            (expected - actual[axis].as_f64().unwrap()).abs() <= 0.75,
                            "{} corner {corner} axis {axis}: {expected} vs {actual}",
                            actor["name"]
                        );
                        checks += 1;
                    }
                }
            }
        }
        assert_eq!(checks, 112);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn queued_visibility_matches_native_actor_updates() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("queued-visibility.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Queued Visibility",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.2,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 1.0 / 60.0,
            max_events: 5000,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(document["runtime_errors"], serde_json::json!([]));
        assert_eq!(document["dropped_events"], 0);
        let native = crate::actor_conformance::evaluate(
            &root.join("fixtures/actors/queued-visibility.json"),
        )
        .unwrap();
        let definitions = document["actor_definitions"].as_array().unwrap();
        let tracks = document["projected_vertex_tracks"].as_array().unwrap();
        let mut checked = 0;
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            for actor in sample["actors"].as_array().unwrap().iter().skip(1) {
                let definition = definitions
                    .iter()
                    .find(|def| def["name"] == actor["name"])
                    .unwrap();
                let track = tracks
                    .iter()
                    .find(|track| track["definition_id"] == definition["id"])
                    .unwrap();
                let actual = track["samples"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|row| row[1].as_f64().unwrap() <= time + 0.00001)
                    .expect("a sparse track retains its state between changes");
                assert_eq!(actual[2], actor["visible"], "{} at {time}", actor["name"]);
                checked += 1;
            }
        }
        assert_eq!(checked, 35);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn message_queue_offsets_match_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
        let entry = song_dir.join("queue-backlog.lua");
        let bpms = [BpmSegment { beat: 0.0, bpm: 60.0 }];
        let context = Context {
            simfile: &entry, song_dir: &song_dir, title: "Queue backlog",
            difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
            max_beat: 3.0, bpm: 60.0, bpm_segments: &bpms, beat_step: 0.25,
            max_events: 5000, random_seed: 1,
        };
        let mut document = evaluate(&[Entry {
            path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
        }], &context).unwrap();
        assert_eq!(document["runtime_errors"], serde_json::json!([]));
        assert_eq!(document["dropped_events"], 0);
        crate::song_lua_semantics::enrich(&mut document).unwrap();
        let native = crate::actor_conformance::evaluate(
            &root.join("fixtures/actors/queue-backlog.json"),
        ).unwrap();
        let backlog = native["samples"][1]["actors"][1]["tween_time_left"].as_f64().unwrap();
        assert!((backlog - 0.76).abs() < 0.00001);
        let queue = native["samples"][2]["actors"][1]["tween_queue"].as_array().unwrap();
        assert_eq!(queue.len(), 4, "native dispatch retains the preceding tween");
        let segments = document["tween_segments"].as_array().unwrap().iter()
            .filter(|segment| segment["command"] == "AppendCommand")
            .collect::<Vec<_>>();
        assert_eq!(segments.len(), 3);
        let mut start = backlog;
        for (segment, tween) in segments.iter().zip(&queue[1..]) {
            assert!((segment["queue_start_seconds"].as_f64().unwrap() - start).abs() < 0.002);
            let duration = tween["duration"].as_f64().unwrap();
            assert!((segment["duration"].as_f64().unwrap() - duration).abs() < 0.00001);
            start += duration;
        }
        let sleep = document["tween_segments"].as_array().unwrap().iter()
            .find(|segment| segment["implicit"] == true).unwrap();
        assert_eq!(sleep["queue_start_seconds"], 2.0);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn pulse_geometry_matches_native() {
        assert_eq!(effect_geometry_matches_native("pulse-body"), 800);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn motion_geometry_matches_native() {
        assert_eq!(effect_geometry_matches_native("motion-body"), 31460);
    }

    #[cfg(itgmania_oracle)]
    fn effect_geometry_matches_native(stem: &str) -> usize {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join(format!("{stem}.lua"));
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: stem,
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 4.0,
            bpm: 120.0,
            bpm_segments: &[],
            beat_step: 1.0 / 60.0,
            max_events: 20000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native =
            crate::actor_conformance::evaluate(&root.join(format!("fixtures/actors/{stem}.json")))
                .unwrap();
        let mut checked = 0;
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            for actor in sample["actors"].as_array().unwrap() {
                let Some(vertices) = actor["draws"][0]["vertices"].as_array() else {
                    continue;
                };
                let runtime = trace["runtime_actors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["name"] == actor["name"])
                    .unwrap();
                let track = trace["projected_vertex_tracks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["actor"] == runtime["id"])
                    .unwrap();
                let actual = track["samples"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .rfind(|row| row[1].as_f64().unwrap() <= time + 0.000001)
                    .unwrap();
                for corner in 0..4 {
                    let vertex = &vertices[[0, 3, 2, 1][corner]];
                    for axis in 0..3 {
                        let got = actual[4][corner][axis].as_f64().unwrap();
                        let want = vertex["world"][axis].as_f64().unwrap();
                        assert!(
                            (got - want).abs() <= 0.00015,
                            "{} at {time}, world {corner}/{axis}: {got} vs {want}",
                            actor["name"]
                        );
                        checked += 1;
                    }
                    for axis in 0..2 {
                        let got = actual[6][corner][axis].as_f64().unwrap();
                        let want = vertex["screen"][axis].as_f64().unwrap();
                        assert!(
                            (got - want).abs() <= 0.0003,
                            "{} at {time}, screen {corner}/{axis}: {got} vs {want}",
                            actor["name"]
                        );
                        checked += 1;
                    }
                }
            }
        }
        checked
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn tween_boundaries_match_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("tween-boundary.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Tween boundaries",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 8.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 1.0 / 60.0,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/tween-boundary.json"))
                .unwrap();
        let track = trace["projected_vertex_tracks"][0]["samples"]
            .as_array()
            .unwrap();
        let mut checked = 0;
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            let actual = track
                .iter()
                .rfind(|row| row[1].as_f64().unwrap() <= time + 0.000001)
                .unwrap();
            let actor = &sample["actors"][1];
            if actor["draws"].as_array().unwrap().is_empty() {
                assert_eq!(actual[2], false);
                continue;
            }
            assert_eq!(actual[2], actor["visible"]);
            for corner in 0..4 {
                for axis in 0..2 {
                    let expected = actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["screen"]
                        [axis]
                        .as_f64()
                        .unwrap();
                    let actual = actual[6][corner][axis].as_f64().unwrap();
                    assert!(
                        (expected - actual).abs() < 0.002,
                        "at {time}, {corner}/{axis}: {expected} vs {actual}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 1000);
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn tween_depth_matches_native() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("near-camera.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "near camera",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 3.0,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 1.0 / 60.0,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/near-camera.json"))
                .unwrap();
        let track = &trace["projected_vertex_tracks"][0]["samples"];
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            let actual = track
                .as_array()
                .unwrap()
                .iter()
                .find(|row| (row[1].as_f64().unwrap() - time).abs() < 0.000001)
                .unwrap();
            for corner in 0..4 {
                let vertex = &sample["actors"][1]["draws"][0]["vertices"][[0, 3, 2, 1][corner]];
                // Near the plane, a one-ULP depth error moves the projection by pixels.
                assert_eq!(
                    (actual[4][corner][2].as_f64().unwrap() as f32).to_bits(),
                    (vertex["world"][2].as_f64().unwrap() as f32).to_bits()
                );
                for axis in 0..2 {
                    assert_eq!(
                        (actual[6][corner][axis].as_f64().unwrap() as f32).to_bits(),
                        (vertex["screen"][axis].as_f64().unwrap() as f32).to_bits()
                    );
                }
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn restarted_vibration_matches_native_effect_state() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("vibration-restart.lua");
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "vibration restart",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 2.5,
            bpm: 60.0,
            bpm_segments: &[],
            beat_step: 0.25,
            max_events: 5000,
            random_seed: 1,
        };
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        let native = crate::actor_conformance::evaluate(
            &root.join("fixtures/actors/vibration-restart.json"),
        )
        .unwrap();
        let samples = trace["projected_vertex_tracks"][0]["samples"]
            .as_array()
            .unwrap();
        let expected = [
            [20, 12, 4],
            [20, 12, 4],
            [10, 10, 10],
            [10, 10, 10],
            [7, 8, 9],
        ];
        for (index, sample) in native["samples"].as_array().unwrap().iter().enumerate() {
            let time = sample["time"].as_f64().unwrap();
            let effect = &sample["actors"][0]["effect"];
            for axis in 0..3 {
                assert_eq!(
                    effect["magnitude"][axis].as_f64().unwrap(),
                    f64::from(expected[index][axis])
                );
            }
            let actual = samples
                .iter()
                .rfind(|row| row[1].as_f64().unwrap() <= time + 0.00001)
                .unwrap();
            let chain = actual[8].as_array().unwrap();
            if index == 1 {
                assert_eq!(effect["type"], "none");
                assert!(chain.is_empty());
            } else {
                assert_eq!(effect["type"], "vibrate");
                assert_eq!(chain.len(), 1);
                assert_eq!(chain[0]["mode"], effect["type"]);
                for axis in 0..3 {
                    assert_eq!(
                        chain[0]["magnitude"][axis].as_f64(),
                        effect["magnitude"][axis].as_f64()
                    );
                }
            }
        }
    }

    #[cfg(itgmania_oracle)]
    #[test]
    fn wrapper_effects_follow_native_draw() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let song_dir = root
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .expect("headless fixtures");
        let entry = song_dir.join("wrapper-effects.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "Wrapper effects",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 1.0,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 1000,
            random_seed: 1,
        };
        // The real C++ Actor::Draw path must move the quad within the same
        // summed envelope. Headless corners intentionally omit random draws.
        let native =
            crate::actor_conformance::evaluate(&root.join("fixtures/actors/wrapper-effects.json"))
                .expect("linked native wrapper drawing");
        let mut moved = false;
        for sample in native["samples"].as_array().expect("native draw frames") {
            let actor = sample["actors"]
                .as_array()
                .expect("native actors")
                .iter()
                .find(|actor| actor["name"] == "Wrapped")
                .expect("wrapped native sprite");
            let vertices = actor["draws"][0]["vertices"]
                .as_array()
                .expect("native quad vertices");
            let center = vertices.iter().fold([0.0; 2], |mut center, vertex| {
                for axis in 0..2 {
                    center[axis] += vertex["world"][axis]
                        .as_f64()
                        .expect("native world coordinate")
                        / vertices.len() as f64;
                }
                center
            });
            let offset = [center[0] - 110.0, center[1] - 85.0];
            assert!(
                offset[0].abs() <= 25.001 && offset[1].abs() <= 32.001,
                "native wrapper envelope: {offset:?}"
            );
            moved |= offset.iter().any(|axis| axis.abs() > 0.001);
        }
        assert!(moved, "native PreDraw must apply wrapper vibration");
        let trace = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .expect("wrapper effect capture");
        assert_eq!(trace["runtime_errors"], serde_json::json!([]));
        assert_eq!(trace["dropped_events"], 0);
        let tracks = trace["projected_vertex_tracks"]
            .as_array()
            .expect("projected actors");
        assert_eq!(tracks.len(), 1);
        for sample in tracks[0]["samples"].as_array().expect("projected samples") {
            let effects = sample[8].as_array().expect("native draw effect chain");
            let magnitudes = effects
                .iter()
                .filter(|effect| effect["mode"] == "vibrate")
                .map(|effect| effect["magnitude"].clone())
                .collect::<Vec<_>>();
            assert_eq!(
                magnitudes,
                vec![
                    serde_json::json!([5, 7, 0]),
                    serde_json::json!([19, 23, 0]),
                    serde_json::json!([1, 2, 0])
                ],
                "Actor::Draw applies each direct wrapper before drawing its owner"
            );
        }
    }
    #[cfg(itgmania_oracle)]
    #[test]
    fn records_orthographic_geometry_and_ancestor_vibration() {
        let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/song-lua-headless")
            .canonicalize()
            .unwrap();
        let entry = song_dir.join("orthographic-vibrate.lua");
        let bpms = [BpmSegment {
            beat: 0.0,
            bpm: 60.0,
        }];
        let context = Context {
            simfile: &entry,
            song_dir: &song_dir,
            title: "orthographic vibration",
            difficulty: "Difficulty_Challenge",
            steps_type: "dance-single",
            description: "",
            max_beat: 0.25,
            bpm: 60.0,
            bpm_segments: &bpms,
            beat_step: 0.25,
            max_events: 100,
            random_seed: 1,
        };
        let document = evaluate(
            &[Entry {
                path: entry.clone(),
                layer: "foreground",
                index: 0,
                start_beat: 0.0,
            }],
            &context,
        )
        .unwrap();
        let tracks = document["projected_vertex_tracks"].as_array().unwrap();
        assert_eq!(tracks.len(), 1);
        let track = &tracks[0];
        assert_eq!(track["class"], "Quad");
        assert_eq!(track["camera_actor"], "orthographic-screen");
        assert_eq!(track["sample_layout"][8], "effect_chain");
        let sample = &track["samples"][0];
        let vertices = sample[6].as_array().unwrap();
        let center = vertices.iter().fold([0.0; 2], |mut center, vertex| {
            center[0] += vertex[0].as_f64().unwrap() / 4.0;
            center[1] += vertex[1].as_f64().unwrap() / 4.0;
            center
        });
        assert!(
            (center[0] - 110.0).abs() < 1e-6,
            "unexpected x center: {center:?}"
        );
        assert!(
            (center[1] - 85.0).abs() < 1e-6,
            "unexpected y center: {center:?}"
        );
        let effect = &sample[8][0];
        assert_eq!(effect["mode"], "vibrate");
        assert_eq!(effect["magnitude"], serde_json::json!([20, 12, 4]));
    }
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn effect_clock_getters_match_native() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let song_dir = root
        .join("tests/fixtures/song-lua-headless")
        .canonicalize()
        .unwrap();
    let entry = song_dir.join("effect-clock.lua");
    let bpms = [BpmSegment {
        beat: 0.0,
        bpm: 120.0,
    }];
    let context = Context {
        simfile: &entry,
        song_dir: &song_dir,
        title: "Effect Clock",
        difficulty: "Difficulty_Challenge",
        steps_type: "dance-single",
        description: "",
        max_beat: 6.0,
        bpm: 120.0,
        bpm_segments: &bpms,
        beat_step: 1.0 / 60.0,
        max_events: 10_000,
        random_seed: 1,
    };
    let trace = evaluate(
        &[Entry {
            path: entry.clone(),
            layer: "foreground",
            index: 0,
            start_beat: 0.0,
        }],
        &context,
    )
    .unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    let native =
        crate::actor_conformance::evaluate(&root.join("fixtures/actors/effect-clock.json"))
            .unwrap();
    let mut checks = 0;
    for track in trace["callback_operation_tracks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|track| track["operation"] == "Quad.xy")
    {
        let definition = trace["actor_definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["id"] == track["actor"])
            .unwrap();
        for event in track["samples"].as_array().unwrap() {
            let second = event[2].as_f64().unwrap() as f32;
            let sample = native["samples"]
                .as_array()
                .unwrap()
                .iter()
                .find(|sample| sample["time"].as_f64().unwrap() as f32 == second)
                .unwrap();
            let actor = sample["actors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|actor| actor["name"] == definition["name"])
                .unwrap();
            assert_eq!(
                event[3][0].as_f64(),
                actor["effect"]["seconds"].as_f64(),
                "{} seconds at {second}",
                definition["name"]
            );
            assert_eq!(
                event[3][1].as_f64(),
                actor["effect"]["delta"].as_f64(),
                "{} delta at {second}",
                definition["name"]
            );
            checks += 2;
        }
    }
    assert_eq!(checks, 724);
}

#[cfg(all(test, itgmania_oracle))]
fn check_music_effect_clock(label: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
    let entry = song_dir.join("music-effect-clock.lua");
    let simfile = song_dir.join(format!("music-effect-{label}.sm"));
    let bpms = [BpmSegment { beat: 0.0, bpm: 120.0 }];
    let context = Context {
        simfile: &simfile, song_dir: &song_dir, title: "Music Effect Clock",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "clock",
        max_beat: 6.0, bpm: 120.0, bpm_segments: &bpms,
        beat_step: 1.0 / 60.0, max_events: 10_000, random_seed: 1,
    };
    let trace = evaluate(&[Entry {
        path: entry, layer: "foreground", index: 0, start_beat: 0.0,
    }], &context).unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    // This oracle calls the compiled Actor::Update implementation; it does not
    // use semantic_host.lua to calculate effect time or spin rotation.
    let native = crate::actor_conformance::evaluate(
        &root.join(format!("fixtures/actors/music-effect-{label}.json")),
    ).unwrap();
    let mut checks = 0;
    for track in trace["callback_operation_tracks"].as_array().unwrap().iter()
        .filter(|track| track["operation"] == "Quad.xy" || track["operation"] == "Quad.z")
    {
        let definition = trace["actor_definitions"].as_array().unwrap().iter()
            .find(|definition| definition["id"] == track["actor"]).unwrap();
        for event in track["samples"].as_array().unwrap() {
            let second = event[2].as_f64().unwrap() as f32;
            let sample = native["samples"].as_array().unwrap().iter()
                .find(|sample| sample["time"].as_f64().unwrap() as f32 == second).unwrap();
            let actor = sample["actors"].as_array().unwrap().iter()
                .find(|actor| actor["name"] == definition["name"]).unwrap();
            let fields = if track["operation"] == "Quad.xy" {
                vec![&actor["effect"]["seconds"], &actor["effect"]["delta"]]
            } else {
                vec![&actor["current"]["rotation"][2]]
            };
            for (index, expected) in fields.into_iter().enumerate() {
                assert_eq!(event[3][index].as_f64(), expected.as_f64(),
                    "{label} {} field {index} at {second}", definition["name"]);
                checks += 1;
            }
        }
    }
    assert_eq!(checks, 1629, "retain all three fields on all 181 frames");
    let mut vertices = 0;
    for track in trace["projected_vertex_tracks"].as_array().unwrap() {
        let definition = trace["actor_definitions"].as_array().unwrap().iter()
            .find(|definition| definition["id"] == track["actor"]).unwrap();
        if !definition["name"].as_str().unwrap_or("").starts_with("Pulse") { continue; }
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            let actual = track["samples"].as_array().unwrap().iter()
                .rfind(|row| row[1].as_f64().unwrap() <= time + 0.000001).unwrap();
            let actor = sample["actors"].as_array().unwrap().iter()
                .find(|actor| actor["name"] == definition["name"]).unwrap();
            for corner in 0..4 {
                for axis in 0..2 {
                    let expected = actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["screen"][axis]
                        .as_f64().unwrap();
                    let actual = actual[6][corner][axis].as_f64().unwrap();
                    assert!((expected - actual).abs() < 0.002,
                        "{label} {} vertex {corner}/{axis} at {time}: {expected} vs {actual}",
                        definition["name"]);
                    vertices += 1;
                }
            }
        }
    }
    assert_eq!(vertices, 4344, "retain every timer and music pulse vertex");
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn positive_music_effect_clock_matches_native() {
    check_music_effect_clock("positive");
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn negative_music_effect_clock_matches_native() {
    check_music_effect_clock("negative");
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn late_pulse_matches_native() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let song_dir = root
        .join("tests/fixtures/song-lua-headless")
        .canonicalize()
        .unwrap();
    let entry = song_dir.join("late-pulse.lua");
    let context = Context {
        simfile: &entry,
        song_dir: &song_dir,
        title: "Late pulse",
        difficulty: "Difficulty_Challenge",
        steps_type: "dance-single",
        description: "",
        max_beat: 14.0,
        bpm: 120.0,
        bpm_segments: &[],
        beat_step: 1.0 / 60.0,
        max_events: 10_000,
        random_seed: 1,
    };
    let trace = evaluate(
        &[Entry {
            path: entry.clone(),
            layer: "foreground",
            index: 0,
            start_beat: 0.0,
        }],
        &context,
    )
    .unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    let native =
        crate::actor_conformance::evaluate(&root.join("fixtures/actors/late-pulse.json")).unwrap();
    let mut checked = 0;
    for track in trace["projected_vertex_tracks"].as_array().unwrap() {
        let definition = trace["actor_definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["id"] == track["actor"])
            .unwrap();
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            let actual = track["samples"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|row| row[1].as_f64().unwrap() <= time + 0.000001)
                .unwrap();
            let actor = sample["actors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|actor| actor["name"] == definition["name"])
                .unwrap();
            for corner in 0..4 {
                for axis in 0..2 {
                    let expected = actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["screen"]
                        [axis]
                        .as_f64()
                        .unwrap();
                    let actual = actual[6][corner][axis].as_f64().unwrap();
                    assert!(
                        (expected - actual).abs() < 0.002,
                        "{} at {time}, {corner}/{axis}: {expected} vs {actual}",
                        definition["name"]
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 6736);
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn effect_switch_matches_native() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let song_dir = root
        .join("tests/fixtures/song-lua-headless")
        .canonicalize()
        .unwrap();
    let entry = song_dir.join("effect-switch.lua");
    let context = Context {
        simfile: &entry,
        song_dir: &song_dir,
        title: "Effect switch",
        difficulty: "Difficulty_Challenge",
        steps_type: "dance-single",
        description: "",
        max_beat: 8.0,
        bpm: 120.0,
        bpm_segments: &[],
        beat_step: 1.0 / 60.0,
        max_events: 20_000,
        random_seed: 1,
    };
    let trace = evaluate(
        &[Entry {
            path: entry.clone(),
            layer: "foreground",
            index: 0,
            start_beat: 0.0,
        }],
        &context,
    )
    .unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    let native =
        crate::actor_conformance::evaluate(&root.join("fixtures/actors/effect-switch.json"))
            .unwrap();
    let mut mode_checks = 0;
    let mut coordinates = 0;
    for track in trace["projected_vertex_tracks"].as_array().unwrap() {
        let definition = trace["actor_definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|definition| definition["id"] == track["actor"])
            .unwrap();
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            let actual = track["samples"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|row| row[1].as_f64().unwrap() <= time + 0.000001)
                .unwrap();
            let actor = sample["actors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|actor| actor["name"] == definition["name"])
                .unwrap();
            let effect_actor = if definition["name"] == "Nested" {
                &sample["actors"][2]
            } else {
                actor
            };
            let vibrating = effect_actor["effect"]["type"] == "vibrate";
            assert_eq!(
                actual[8]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|effect| effect["mode"] == "vibrate"),
                vibrating,
                "{} at {time}",
                definition["name"]
            );
            mode_checks += 1;
            if vibrating {
                continue;
            }
            for corner in 0..4 {
                for axis in 0..2 {
                    let expected = actor["draws"][0]["vertices"][[0, 3, 2, 1][corner]]["screen"]
                        [axis]
                        .as_f64()
                        .unwrap();
                    let actual = actual[6][corner][axis].as_f64().unwrap();
                    assert!(
                        (expected - actual).abs() < 0.002,
                        "{} at {time}, {corner}/{axis}: {expected} vs {actual}",
                        definition["name"]
                    );
                    coordinates += 1;
                }
            }
        }
    }
    assert_eq!(mode_checks, 482);
    assert_eq!(coordinates, 2896);
    eprintln!(
        "effect switch: {mode_checks} native vibration selectors, {coordinates} native coordinates"
    );
}

#[cfg(all(test, itgmania_oracle))]
fn native_draw_color_checks(fixture: &str, max_beat: f32) -> usize {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let song_dir = root
        .join("tests/fixtures/song-lua-headless")
        .canonicalize()
        .unwrap();
    let entry = song_dir.join(format!("{fixture}.lua"));
    let context = Context {
        simfile: &entry,
        song_dir: &song_dir,
        title: "Color inheritance",
        difficulty: "Difficulty_Challenge",
        steps_type: "dance-single",
        description: "",
        max_beat,
        bpm: 60.0,
        bpm_segments: &[],
        beat_step: 1.0 / 60.0,
        max_events: 20_000,
        random_seed: 1,
    };
    let trace = evaluate(
        &[Entry {
            path: entry.clone(),
            layer: "foreground",
            index: 0,
            start_beat: 0.0,
        }],
        &context,
    )
    .unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    assert_eq!(trace["capabilities"]["projected_draw_color_samples"], true);
    assert_eq!(
        trace["semantic_derivation"]["render_model"]["projected_sample_layout"],
        trace["projected_vertex_tracks"][0]["sample_layout"]
    );
    let native =
        crate::actor_conformance::evaluate(&root.join(format!("fixtures/actors/{fixture}.json")))
            .unwrap();
    let mut checked = 0;
    for track in trace["projected_vertex_tracks"].as_array().unwrap() {
        let definition = trace["actor_definitions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["id"] == track["actor"])
            .unwrap();
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            let actual = track["samples"]
                .as_array()
                .unwrap()
                .iter()
                .rfind(|row| row[1].as_f64().unwrap() <= time + 0.000001)
                .unwrap();
            let actor = sample["actors"]
                .as_array()
                .unwrap()
                .iter()
                .find(|a| a["name"] == definition["name"])
                .unwrap();
            let draws = actor["draws"].as_array().unwrap();
            assert!(!draws.is_empty());
            assert_eq!(actual[2], true);
            for draw in draws {
                let field = if draw["texture_mode"] == "glow" {
                    10
                } else {
                    9
                };
                for channel in 0..4 {
                    let expected =
                        draw["vertices"][0]["color"][channel].as_u64().unwrap() as f64 / 255.0;
                    let value = actual[field][channel].as_f64().unwrap();
                    assert!(
                        (expected - value).abs() <= 1.0 / 255.0 + 0.00001,
                        "{} at {time} field {field} channel {channel}: {expected} != {value}",
                        definition["name"]
                    );
                    checked += 1;
                }
            }
            assert_eq!(actual[3], actual[9][3]);
        }
    }
    checked
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn draw_colors_match_native() {
    assert_eq!(native_draw_color_checks("color-inheritance", 2.0), 13664);
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn late_draw_colors_match_native() {
    assert_eq!(native_draw_color_checks("late-colors", 4.0), 14460);
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn actor_lookup_matches_itg() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let native = crate::actor_conformance::evaluate(&root.join("fixtures/actors/child-lookup.json"))
        .expect("actual native ActorFrame Lua lookup assertions");
    assert_eq!(native["script_errors"], serde_json::json!([]));
    assert_eq!(native["allocations"], serde_json::json!([]));
    let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize()
        .expect("actor lookup fixture folder");
    let entry = song_dir.join("actor-lookup.lua");
    let context = Context {
        simfile: &entry, song_dir: &song_dir, title: "native actor lookup",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
        max_beat: 0.25, bpm: 60.0, bpm_segments: &[], beat_step: 0.25,
        max_events: 10000, random_seed: 1,
    };
    let trace = evaluate(&[Entry {path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0}], &context)
        .expect("song ActorFrame lookup capture");
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    assert_eq!(trace["runtime_actors"].as_array().expect("song actors").len(), 5);
    assert!(trace.to_string().contains("ActorFrame.aux"));
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn actor_methods_match_itg() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let native = crate::actor_conformance::evaluate(&root.join("fixtures/actors/method-probes.json"))
        .expect("actual native actor method feature probes");
    assert_eq!(native["script_errors"], serde_json::json!([]));
    let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize()
        .expect("method probe fixture folder");
    let entry = song_dir.join("method-probes.lua");
    let context = Context {
        simfile: &entry, song_dir: &song_dir, title: "native method probes",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
        max_beat: 0.25, bpm: 60.0, bpm_segments: &[], beat_step: 0.25,
        max_events: 10000, random_seed: 1,
    };
    let trace = evaluate(&[Entry {path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0}], &context)
        .expect("actor method probe capture");
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn child_tables_follow_native_insertion_and_group_rules() {
    let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/song-lua-headless")
        .canonicalize()
        .unwrap();
    let entry = song_dir.join("child-table.lua");
    let context = Context {
        simfile: &entry,
        song_dir: &song_dir,
        title: "child table",
        difficulty: "Difficulty_Challenge",
        steps_type: "dance-single",
        description: "",
        max_beat: 0.25,
        bpm: 60.0,
        bpm_segments: &[],
        beat_step: 0.25,
        max_events: 10000,
        random_seed: 1,
    };
    let trace = evaluate(
        &[Entry {
            path: entry.clone(),
            layer: "foreground",
            index: 0,
            start_beat: 0.0,
        }],
        &context,
    )
    .unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    assert!(trace.to_string().contains("ChildTableChecked"));
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn manual_draws_keep_each_frame_and_color_pass() {
    let song_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/song-lua-headless")
        .canonicalize()
        .unwrap();
    let entry = song_dir.join("manual-draw.lua");
    let context = Context {
        simfile: &entry,
        song_dir: &song_dir,
        title: "manual draw",
        difficulty: "Difficulty_Challenge",
        steps_type: "dance-single",
        description: "",
        max_beat: 2.0,
        bpm: 60.0,
        bpm_segments: &[],
        beat_step: 0.25,
        max_events: 10000,
        random_seed: 1,
    };
    let trace = evaluate(
        &[Entry {
            path: entry.clone(),
            layer: "foreground",
            index: 0,
            start_beat: 0.0,
        }],
        &context,
    )
    .unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    let frames = trace["manual_draw_frames"].as_array().unwrap();
    assert_eq!(frames.len(), 121);
    for (index, frame) in frames.iter().enumerate() {
        assert!((frame[0].as_f64().unwrap() - index as f64 / 60.0).abs() < 1e-5);
        let calls = frame[2].as_array().unwrap();
        assert_eq!(calls.len(), 6);
        assert_eq!(calls[0]["operation"], "begin");
        assert_eq!(calls[0]["logical_size"], serde_json::json!([640, 480]));
        assert_eq!(calls[0]["backing_size"], serde_json::json!([1024, 512]));
        assert_eq!(calls[0]["preserve"], index >= 60);
        assert_eq!(calls[0]["alpha"], true);
        assert_eq!(calls[0]["depth"], false);
        assert_eq!(calls[0]["float"], true);
        assert_eq!(calls[4]["operation"], "finish");
        assert_eq!(calls[0]["target"], calls[4]["target"]);
        assert_eq!(calls[5]["operation"], "draw");
        assert_eq!(calls[5]["target"], "screen");
        assert_eq!(calls[5]["path"], "ScreenGameplay/PlayerP1");
        for pass in 0..3 {
            let call = &calls[pass + 1];
            assert_eq!(call["target"], calls[0]["target"]);
            assert_eq!(
                call["texture"],
                format!("aft:{}", calls[0]["target"].as_str().unwrap())
            );
            let primitives = call["primitives"].as_array().unwrap();
            assert_eq!(primitives.len(), 1);
            assert_eq!(primitives[0]["primitive"], "quad_strip");
            let vertices = primitives[0]["vertices"].as_array().unwrap();
            assert_eq!(vertices.len(), 4);
            assert!(
                (vertices[0]["screen"][0].as_f64().unwrap()
                    - index as f64 / 60.0
                    - pass as f64 * 20.0)
                    .abs()
                    < 1e-5
            );
            assert_eq!(vertices[0]["color"][pass], 255);
            assert_eq!(vertices[0]["color"][(pass + 1) % 3], 0);
            for axis in 0..2 {
                assert_eq!(vertices[3]["uv"][axis].as_f64(), Some(1.0));
            }
        }
        assert!(
            (calls[3]["primitives"][0]["vertices"][0]["local"][1]
                .as_f64()
                .unwrap()
                - index as f64 / 60.0)
                .abs()
                < 1e-5
        );
    }
    // The first pass keeps its prior vertex data when the shared Lua table is
    // mutated for the third pass, and earlier frames remain immutable.
    assert_eq!(
        frames[0][2][1]["primitives"][0]["vertices"][0]["local"][1].as_f64(),
        Some(0.0)
    );
    assert!(
        (frames[120][2][1]["primitives"][0]["vertices"][0]["local"][1]
            .as_f64()
            .unwrap()
            - frames[119][0].as_f64().unwrap())
        .abs()
            < 1e-5
    );
}

#[cfg(all(test, itgmania_oracle))]
fn model_json_numbers(value: &Value) -> Value {
    // Lua's JSON encoder writes integral doubles as integers. Compare the
    // same numeric values without changing floating-point tolerances.
    match value {
        Value::Number(number) => serde_json::json!(number.as_f64().unwrap()),
        Value::Array(items) => Value::Array(items.iter().map(model_json_numbers).collect()),
        Value::Object(fields) => Value::Object(fields.iter()
            .map(|(key,value)| (key.clone(),model_json_numbers(value))).collect()),
        _ => value.clone(),
    }
}

#[cfg(all(test, itgmania_oracle))]
fn model_expand_draws(trace: &Value, draws: &Value) -> Vec<Value> {
    assert_eq!(trace["model_geometry_encoding"], "column-buffer-v1");
    let buffers = trace["model_geometry_buffers"].as_array().expect("Model buffers");
    let resolve = |id: &Value| {
        let index = id.as_u64().expect("positive Model buffer ID").checked_sub(1)
            .expect("one-based Model buffer ID");
        buffers.get(index as usize).expect("existing Model buffer").as_array()
            .expect("Model column array")
    };
    draws.as_array().expect("Model primitives").iter().map(|draw| {
        let mut draw = draw.as_object().expect("Model primitive object").clone();
        let count = draw.remove("vertex_count").expect("vertex count").as_u64().unwrap() as usize;
        let fields = draw.remove("vertex_buffers").expect("vertex buffers");
        assert_eq!(fields.as_object().unwrap().len(), 9);
        let mut vertices = vec![serde_json::Map::new(); count];
        for (field, id) in fields.as_object().unwrap() {
            let column = resolve(id);
            assert_eq!(column.len(), count, "{field} column length");
            for (vertex, value) in vertices.iter_mut().zip(column) {
                vertex.insert(field.clone(), value.clone());
            }
        }
        assert!(draw.insert("vertices".into(), Value::Array(vertices.into_iter().map(Value::Object).collect())).is_none());
        for (field, reference) in [("normals", "normals_buffer"),
            ("texture_matrix_scale", "texture_matrix_scale_buffer")] {
            let id = draw.remove(reference).expect("mesh attribute buffer");
            let column = resolve(&id);
            assert_eq!(column.len(), count, "{field} column length");
            assert!(draw.insert(field.into(), Value::Array(column.clone())).is_none());
        }
        Value::Object(draw)
    }).collect()
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn model_song_meshes_match_native() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let native = crate::actor_conformance::evaluate(&root.join("fixtures/actors/model-song-geometry.json"))
        .expect("actual native Model geometry control");
    let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
    let entry = song_dir.join("model-geometry.lua");
    let context = Context {
        simfile: &entry, song_dir: &song_dir, title: "native song Models",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
        max_beat: 1.0, bpm: 60.0, bpm_segments: &[], beat_step: 0.25,
        max_events: 10000, random_seed: 1,
    };
    let trace = evaluate_with_noteskin(&[Entry {
        path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
    }], &context, None).expect("song native mesh capture");
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    let tracks = trace["model_geometry_tracks"].as_array().expect("Model tracks");
    assert_eq!(trace["model_geometry_sample_clock"], "update_frames");
    let frames = trace["update_frames"].as_array().unwrap();
    assert_eq!(frames.len(), 61);
    for track in tracks {
        let samples = track["samples"].as_array().unwrap();
        assert_eq!(samples.len(), frames.len(), "every Model update must be observed");
        for (sample, frame) in samples.iter().zip(frames) {
            assert_eq!(sample.as_array().unwrap()[..2], frame.as_array().unwrap()[..]);
        }
    }
    assert!(trace["model_geometry_buffer_stats"]["hits"].as_u64().unwrap() > 0);
    assert_eq!(trace["model_geometry_buffer_stats"]["saturated_misses"], 0);
    assert_eq!(trace["model_geometry_buffer_stats"]["buffers"].as_u64().unwrap() as usize,
        trace["model_geometry_buffers"].as_array().unwrap().len());
    assert_eq!(tracks.len(), 2);
    assert!(!tracks[1]["samples"].as_array().unwrap().is_empty());
    for sample in tracks[1]["samples"].as_array().unwrap() {
        assert_eq!(sample[2], false);
        assert_eq!(sample[3], serde_json::json!([]));
    }
    for sample in native["samples"].as_array().unwrap() {
        let time = sample["time"].as_f64().unwrap();
        let captured = tracks[0]["samples"].as_array().unwrap().iter()
            .find(|row| (row[1].as_f64().unwrap()-time).abs() < 0.00001).expect("matching sample");
        let expected = sample["actors"][1]["draws"].as_array().unwrap();
        let actual = model_expand_draws(&trace, &captured[3]);
        check_model_draws(&actual, expected, time);
    }
}

#[cfg(all(test, itgmania_oracle))]
fn check_model_draws(actual: &[Value], expected: &[Value], time: f64) {
    assert_eq!(expected.len(), actual.len());
    for (actual, expected) in actual.iter().zip(expected) {
        for field in ["primitive", "texture_mode", "blend_mode", "model_mesh_name",
            "normals", "texture_matrix_scale", "texture_matrix", "material", "lighting",
            "lights", "cull_mode", "z_write", "z_test", "texture_dimensions"] {
            assert_eq!(model_json_numbers(&actual[field]), model_json_numbers(&expected[field]), "{field} at {time}");
        }
        let texture_path = |value: &Value| value.as_str().map(|name| {
            let path = name.strip_prefix("song:/").map_or_else(|| PathBuf::from(name), |name|
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/song-lua-headless").join(name));
            path.canonicalize().expect("native bound texture exists")
        });
        assert_eq!(texture_path(&actual["texture"]), texture_path(&expected["texture"]), "texture at {time}");
        for field in ["texture_filtering", "texture_wrapping", "sphere_environment", "texture_request"] {
            assert_eq!(actual[field], expected[field], "{field} at {time}");
        }
        assert_eq!(actual["vertices"].as_array().unwrap().len(), expected["vertices"].as_array().unwrap().len());
        for (actual, expected) in actual["vertices"].as_array().unwrap().iter()
            .zip(expected["vertices"].as_array().unwrap()) {
            for field in ["local", "uv", "transformed_uv", "color"] {
                assert_eq!(model_json_numbers(&actual[field]), model_json_numbers(&expected[field]), "{field} at {time}");
            }
            for field in ["world", "view", "clip", "ndc", "screen"] {
                assert_eq!(actual[field].as_array().unwrap().len(), expected[field].as_array().unwrap().len());
                for (a,b) in actual[field].as_array().unwrap().iter().zip(expected[field].as_array().unwrap()) {
                    assert!((a.as_f64().unwrap()-b.as_f64().unwrap()).abs() < 0.0001, "{field} at {time}: {a} != {b}");
                }
            }
        }
    }
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn model_song_base_rotation_matches_native() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let native = crate::actor_conformance::evaluate(&root.join("fixtures/actors/model-base-rotation.json"))
        .expect("native Actor base rotation control");
    let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
    let entry = song_dir.join("model-base-rotation.lua");
    let context = Context {
        simfile: &entry, song_dir: &song_dir, title: "native Model base rotations",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
        max_beat: 1.0, bpm: 60.0, bpm_segments: &[], beat_step: 0.25,
        max_events: 10000, random_seed: 1,
    };
    let trace = evaluate_with_noteskin(&[Entry {
        path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
    }], &context, None).expect("song Model base rotations");
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    let tracks = trace["model_geometry_tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 3);
    assert_eq!(trace["update_frames"].as_array().unwrap().len(), 61);
    let mut passes = 0;
    for (index, track) in tracks.iter().enumerate() {
        assert_eq!(track["samples"].as_array().unwrap().len(), 61);
        for sample in native["samples"].as_array().unwrap() {
            let time = sample["time"].as_f64().unwrap();
            let captured = track["samples"].as_array().unwrap().iter()
                .find(|row| (row[1].as_f64().unwrap()-time).abs() < 0.00001).expect("matching native clock");
            let expected = sample["actors"][index+1]["draws"].as_array().unwrap();
            let actual = model_expand_draws(&trace, &captured[3]);
            check_model_draws(&actual, expected, time);
            passes += actual.len();
        }
    }
    assert_eq!(passes, 30);
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn model_song_texture_commands_match_native() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let native = crate::actor_conformance::evaluate(&root.join("fixtures/actors/model-texture-order.json"))
        .expect("native queued Model states and parent clocks");
    let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
    let entry = song_dir.join("model-texture-order.lua");
    let context = Context {
        simfile: &entry, song_dir: &song_dir, title: "native Model texture command order",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
        max_beat: 2.0, bpm: 60.0, bpm_segments: &[], beat_step: 0.25,
        max_events: 10000, random_seed: 1,
    };
    let trace = evaluate_with_noteskin(&[Entry {
        path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
    }], &context, None).expect("song Model texture command capture");
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    let tracks = trace["model_geometry_tracks"].as_array().unwrap();
    let frames = native["samples"].as_array().unwrap();
    assert_eq!(frames.len(), 121);
    assert_eq!(tracks.len(), 4);
    for (track, name) in tracks.iter().zip(["Queued", "Sleeping", "SleepingChild", "FastChild"]) {
        let captures = track["samples"].as_array().unwrap();
        assert_eq!(captures.len(), frames.len());
        for (captured, sample) in captures.iter().zip(frames) {
            let time = sample["time"].as_f64().unwrap();
            assert!((captured[1].as_f64().unwrap() - time).abs() < 0.000_001);
            let actor = sample["actors"].as_array().unwrap().iter()
                .find(|actor| actor["name"] == name).expect("native Model actor");
            let actual = model_expand_draws(&trace, &captured[3]);
            check_model_draws(&actual, actor["draws"].as_array().unwrap(), time);
        }
    }
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn model_texture_metadata() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let native = crate::actor_conformance::evaluate(&root.join("fixtures/actors/model-texture-request.json"))
        .expect("native Model metadata control");
    let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
    let entry = song_dir.join("model-texture-request.lua");
    let context = Context {
        simfile: &entry, song_dir: &song_dir, title: "native Model texture metadata",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
        max_beat: 0.1, bpm: 60.0, bpm_segments: &[], beat_step: 0.1,
        max_events: 10000, random_seed: 1,
    };
    let trace = evaluate_with_noteskin(&[Entry {
        path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
    }], &context, None).expect("song metadata capture");
    assert_eq!(trace["capabilities"]["model_texture_metadata"], true);
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    let tracks = trace["model_geometry_tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    let samples = tracks[0]["samples"].as_array().unwrap();
    assert!(!samples.is_empty());
    for sample in samples {
        let draws = model_expand_draws(&trace, &sample[3]);
        let expected = native["samples"][0]["actors"][1]["draws"].as_array().unwrap();
        assert_eq!(draws.len(), 3);
        for (draw, reference) in draws.iter().zip(expected) {
            assert_eq!(draw["texture_request"], reference["texture_request"]);
            assert_eq!(model_json_numbers(&draw["texture_dimensions"]),
                model_json_numbers(&reference["texture_dimensions"]));
        }
    }
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn model_song_texture_bindings_match_native() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let native = crate::actor_conformance::evaluate(&root.join("fixtures/actors/model-texture-images.json"))
        .expect("native animated diffuse and additive materials");
    let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
    let entry = song_dir.join("model-texture-images.lua");
    let context = Context {
        simfile: &entry, song_dir: &song_dir, title: "native Model texture bindings",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
        max_beat: 1.0, bpm: 60.0, bpm_segments: &[], beat_step: 0.25,
        max_events: 10000, random_seed: 1,
    };
    let trace = evaluate_with_noteskin(&[Entry {
        path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
    }], &context, None).expect("song Model material image capture");
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
    assert_eq!(trace["capabilities"]["model_texture_bindings"], true);
    let tracks = trace["model_geometry_tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 1);
    let samples = tracks[0]["samples"].as_array().unwrap();
    assert_eq!(samples.len(), 61);
    let mut selected = std::collections::BTreeSet::new();
    for (captured, sample) in samples.iter().zip(native["samples"].as_array().unwrap()) {
        let time = sample["time"].as_f64().unwrap();
        assert!((captured[1].as_f64().unwrap() - time).abs() < 0.000_001);
        let draws = sample["actors"][1]["draws"].as_array().unwrap();
        assert_eq!(draws.len(), 3, "native desktop diffuse, additive and glow passes");
        assert_eq!(draws.iter().map(|draw| draw["blend_mode"].as_u64().unwrap()).collect::<Vec<_>>(), [0, 1, 0]);
        assert_eq!(draws.iter().map(|draw| draw["texture_filtering"].as_bool().unwrap()).collect::<Vec<_>>(), [false, true, false]);
        for draw in draws { selected.insert(Path::new(draw["texture"].as_str().unwrap()).file_name().unwrap().to_string_lossy().into_owned()); }
        let actual = model_expand_draws(&trace, &captured[3]);
        check_model_draws(&actual, draws, time);
    }
    assert_eq!(selected, ["alpha-green.png", "alpha-white.png", "frame-blue.png", "frame-red.png"]
        .into_iter().map(str::to_string).collect());
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn model_song_manual_draws_keep_native_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let native = crate::actor_conformance::evaluate(&root.join("fixtures/actors/model-geometry.json")).unwrap();
    let song_dir = root.join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
    let entry = song_dir.join("model-manual.lua");
    let context = Context {
        simfile: &entry, song_dir: &song_dir, title: "native manual Models",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
        max_beat: 0.1, bpm: 60.0, bpm_segments: &[], beat_step: 0.1,
        max_events: 10000, random_seed: 1,
    };
    let trace = evaluate_with_noteskin(&[Entry {
        path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
    }], &context, None).unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert!(!trace["manual_draw_frames"].as_array().unwrap().is_empty());
    assert!(trace["model_geometry_buffer_stats"]["hits"].as_u64().unwrap() > 0);
    for frame in trace["manual_draw_frames"].as_array().unwrap() {
        let calls = frame[2].as_array().unwrap();
        assert_eq!(calls.len(), 2);
        let first = model_expand_draws(&trace, &calls[0]["primitives"]);
        assert_eq!(first.len(), 2);
        for (actual, expected) in first.iter().zip(native["samples"][0]["actors"][1]["draws"].as_array().unwrap()) {
            assert_eq!(model_json_numbers(&actual["material"]), model_json_numbers(&expected["material"]));
            assert_eq!(actual["vertices"].as_array().unwrap().len(), expected["vertices"].as_array().unwrap().len());
            for (a,b) in actual["vertices"].as_array().unwrap().iter().zip(expected["vertices"].as_array().unwrap()) {
                assert_eq!(model_json_numbers(&a["world"]), model_json_numbers(&b["world"]));
            }
        }
        let second = model_expand_draws(&trace, &calls[1]["primitives"]);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0]["vertices"][0]["world"][0].as_f64(), Some(120.0));
    }
}

#[cfg(all(test, itgmania_oracle))]
#[test]
fn model_song_methods_use_native_contracts() {
    let song_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/song-lua-headless").canonicalize().unwrap();
    let entry = song_dir.join("model-methods.lua");
    let context = Context {
        simfile: &entry, song_dir: &song_dir, title: "native Model methods",
        difficulty: "Difficulty_Challenge", steps_type: "dance-single", description: "",
        max_beat: 0.1, bpm: 60.0, bpm_segments: &[], beat_step: 0.1,
        max_events: 10000, random_seed: 1,
    };
    let trace = evaluate_with_noteskin(&[Entry {
        path: entry.clone(), layer: "foreground", index: 0, start_beat: 0.0,
    }], &context, None).unwrap();
    assert_eq!(trace["runtime_errors"], serde_json::json!([]));
    assert_eq!(trace["dropped_events"], 0);
}
