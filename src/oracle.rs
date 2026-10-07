use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAGIC: &[u8; 8] = b"ITGORCL\0";
pub const SCHEMA_VERSION: u32 = 5;
static INPUT_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Serialize)]
pub struct Document {
    pub schema_version: u32,
    pub simfile: String,
    pub source_sha256: String,
    pub loading_mode: &'static str,
    pub diagnostics: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<ThemeSource>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repair: Option<Repair>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub theme_repairs: Vec<ThemeRepair>,
    pub charts: Vec<Chart>,
}

#[derive(Debug, Serialize)]
pub struct Repair {
    pub original_sha256: String,
    pub oracle_simfile: String,
    pub oracle_sha256: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ThemeRepair {
    pub source_index: u32,
    pub oracle_simfile: String,
    pub oracle_index: u32,
    pub oracle_sha256: String,
    pub reason: String,
}

#[derive(Deserialize)]
struct ThemeRepairManifest {
    version: u32,
    cases: Vec<ThemeRepairCase>,
}
#[derive(Deserialize)]
struct ThemeRepairCase {
    source_sha256: String,
    source_index: u32,
    oracle_simfile: PathBuf,
    #[serde(default)]
    oracle_index: u32,
    oracle_sha256: String,
    reason: String,
}

// Repairs affect only the theme's read surface. The original engine capture is
// authoritative, and a repaired/isolate input must reproduce it byte for byte.
pub fn apply_theme_repairs(
    document: &mut Document,
    manifest: &Path,
    theme: &Path,
) -> Result<(), Error> {
    let bytes = fs::read(manifest)
        .map_err(|e| Error::Native(format!("read theme repair manifest: {e}")))?;
    let config: ThemeRepairManifest = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Native(format!("parse theme repair manifest: {e}")))?;
    if config.version != 1 {
        return Err(Error::Native(
            "unsupported theme repair manifest version".into(),
        ));
    }
    for case in config
        .cases
        .into_iter()
        .filter(|case| case.source_sha256 == document.source_sha256)
    {
        if case.reason.trim().is_empty()
            || document
                .theme_repairs
                .iter()
                .any(|r| r.source_index == case.source_index)
        {
            return Err(Error::Native(
                "theme repairs require a reason and a unique native source index".into(),
            ));
        }
        let path = manifest
            .parent()
            .unwrap_or(Path::new("."))
            .join(&case.oracle_simfile);
        if sha256(&source_bytes(&path)?) != case.oracle_sha256 {
            return Err(Error::Native(format!(
                "theme repair SHA256 does not match {}",
                path.display()
            )));
        }
        let repaired = load_with_theme(&path, Some(theme))?;
        let selected = repaired
            .charts
            .iter()
            .find(|c| c.source_index == case.oracle_index)
            .ok_or_else(|| Error::Native("theme repair oracle index does not exist".into()))?;
        let original = document
            .charts
            .iter_mut()
            .find(|c| c.source_index == case.source_index)
            .ok_or_else(|| Error::Native("theme repair source index does not exist".into()))?;
        if engine_metrics(original)? != engine_metrics(selected)? {
            return Err(Error::Native(format!(
                "theme repair changes engine chart data at source index {}",
                case.source_index
            )));
        }
        if selected
            .theme
            .as_ref()
            .is_none_or(|t| t["status"] != "complete")
        {
            return Err(Error::Native(format!(
                "theme repair remains incomplete at source index {}",
                case.source_index
            )));
        }
        original.theme = selected.theme.clone();
        document.theme_repairs.push(ThemeRepair {
            source_index: case.source_index,
            oracle_simfile: path.to_string_lossy().replace('\\', "/"),
            oracle_index: case.oracle_index,
            oracle_sha256: case.oracle_sha256,
            reason: case.reason,
        });
    }
    Ok(())
}

fn engine_metrics(chart: &Chart) -> Result<Vec<u8>, Error> {
    // Serialization preserves all raw f32 spellings/bits, including signed zero
    // and non-finite values. No numeric tolerance is applied to this proof.
    serde_json::to_vec(&(
        &chart.steps_type,
        chart.difficulty_code,
        chart.meter,
        &chart.note_data,
        chart.note_data_supported,
        &chart.timing,
        &chart.bpm,
        &chart.player_stats,
        &chart.groove_stats_hash,
        chart.groove_stats_hash_version,
        &chart.hash_bpms,
        &chart.first_second,
        &chart.last_second,
    ))
    .map_err(|e| Error::Native(format!("serialize engine equivalence proof: {e}")))
}

#[derive(Debug, Serialize)]
pub struct ThemeSource {
    pub path: String,
    pub scripts: std::collections::BTreeMap<String, String>,
    pub host_sha256: String,
}

#[derive(Debug, Serialize)]
pub struct Chart {
    pub source_index: u32,
    pub title: NativeText,
    pub subtitle: NativeText,
    pub artist: NativeText,
    pub title_translit: NativeText,
    pub subtitle_translit: NativeText,
    pub artist_translit: NativeText,
    pub metadata_utf8: bool,
    pub credit: String,
    pub description: String,
    pub steps_type: StepsType,
    pub difficulty_code: i32,
    pub meter: i32,
    pub bpm: BpmRange,
    pub timing: Timing,
    pub note_data: NoteData,
    pub note_data_supported: bool,
    pub player_stats: Vec<PlayerStats>,
    pub groove_stats_hash: String,
    pub groove_stats_hash_version: i32,
    pub hash_bpms: String,
    pub first_second: Option<RawF32>,
    pub last_second: Option<RawF32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct StepsType {
    pub code: i32,
    pub source: String,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum NativeText {
    Utf8(String),
    Bytes { raw_bytes_hex: String },
}

#[derive(Debug, Serialize)]
pub struct BpmRange {
    pub actual_min: RawF32,
    pub actual_max: RawF32,
    pub display_min: RawF32,
    pub display_max: RawF32,
}

#[derive(Debug, Serialize)]
pub struct Timing {
    pub beat0_offset_seconds: RawF32,
    pub beat0_group_offset_seconds: RawF32,
    pub segments: Vec<TimingSegment>,
}

#[derive(Debug, Serialize)]
pub struct TimingSegment {
    pub segment_type_code: i32,
    pub row: i32,
    pub beat: RawF32,
    pub values: Vec<RawF32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct NoteData {
    pub tracks: u32,
    pub entries: Vec<Note>,
}

#[derive(Debug, Serialize)]
pub struct Note {
    pub row: i32,
    pub beat: RawF32,
    pub track: i32,
    pub note_type_code: i32,
    pub sub_type_code: i32,
    pub source_code: i32,
    pub player_code: i32,
    pub duration_rows: i32,
    pub duration_beats: RawF32,
    pub attack_duration_seconds: RawF32,
    pub keysound_index: i32,
    pub attack_modifiers: String,
}

#[derive(Debug, Serialize)]
pub struct PlayerStats {
    pub player_code: i32,
    pub radar: Vec<RawF32>,
    pub tech_counts: Vec<RawF32>,
    pub notes_per_measure: Vec<i32>,
    pub nps_per_measure: Vec<RawF32>,
    pub peak_nps: RawF32,
}

#[derive(Debug, Clone, Copy)]
pub struct RawF32(f32);

impl RawF32 {
    pub(crate) const fn from_bits(bits: u32) -> Self {
        Self(f32::from_bits(bits))
    }

    pub(crate) const fn get(self) -> f32 {
        self.0
    }
}

impl Serialize for RawF32 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.0.is_finite() {
            return serializer.serialize_f32(self.0);
        }

        let kind = if self.0.is_nan() {
            "nan"
        } else if self.0.is_sign_positive() {
            "positive_infinity"
        } else {
            "negative_infinity"
        };
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("non_finite", kind)?;
        map.serialize_entry("bits", &format!("0x{:08x}", self.0.to_bits()))?;
        map.end()
    }
}

pub fn load(path: &Path) -> Result<Document, Error> {
    load_with_theme(path, None)
}

pub fn source_bytes(path: &Path) -> Result<Vec<u8>, Error> {
    let bytes = fs::read(path)
        .map_err(|error| Error::Native(format!("read {}: {error}", path.display())))?;
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zst"))
    {
        zstd::stream::decode_all(bytes.as_slice())
            .map_err(|error| Error::Native(format!("decompress {}: {error}", path.display())))
    } else {
        Ok(bytes)
    }
}

pub fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn load_with_theme(path: &Path, theme_path: Option<&Path>) -> Result<Document, Error> {
    let bytes = source_bytes(path)?;
    let theme = theme_path
        .map(|root| {
            let mut scripts = std::collections::BTreeMap::new();
            for file in [
                "SL-ChartParser.lua",
                "SL-ChartParserHelpers.lua",
                "SL-BPMDisplayHelpers.lua",
            ] {
                let relative = format!("Scripts/{file}");
                let bytes = fs::read(root.join(&relative))
                    .map_err(|error| Error::Native(format!("read theme {relative}: {error}")))?;
                scripts.insert(relative, sha256(&bytes));
            }
            Ok::<_, Error>(ThemeSource {
                path: root.to_string_lossy().replace('\\', "/"),
                scripts,
                host_sha256: sha256(include_bytes!("../chart_theme.lua")),
            })
        })
        .transpose()?;
    // Give the native loader and Lua the same immutable bytes, with the original
    // extension. Isolated staging also handles compressed corpus files safely.
    let staging = StagedInput::new(path, &bytes)?;
    let (charts, mut diagnostics) = native_load(&staging.path, theme_path)?;
    for diagnostic in &mut diagnostics {
        *diagnostic = diagnostic.replace(
            &staging.path.to_string_lossy().replace('\\', "/"),
            &path.to_string_lossy().replace('\\', "/"),
        );
        *diagnostic = diagnostic.replace(
            staging.path.to_string_lossy().as_ref(),
            path.to_string_lossy().as_ref(),
        );
    }
    Ok(Document {
        schema_version: SCHEMA_VERSION,
        simfile: path.to_string_lossy().into_owned(),
        source_sha256: sha256(&bytes),
        loading_mode: "direct_simfile_no_cache",
        theme_repairs: Vec::new(),
        diagnostics,
        theme,
        repair: None,
        charts,
    })
}

struct StagedInput {
    path: PathBuf,
}
impl StagedInput {
    fn new(source: &Path, bytes: &[u8]) -> Result<Self, Error> {
        // A crashed child can leave a directory behind, and Windows reuses PIDs.
        // Allocate another directory rather than failing a later corpus input.
        let dir = loop {
            let id = INPUT_ID.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir().join(format!("itg-chart-{}-{id}", std::process::id()));
            match fs::create_dir(&dir) {
                Ok(()) => break dir,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(Error::Native(format!("create chart staging: {error}"))),
            }
        };
        let name = if source
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("zst"))
        {
            source.with_extension("")
        } else {
            source.to_owned()
        };
        let filename = name.file_name().unwrap_or_default();
        // RageFile's narrow Windows file API cannot open every UTF-8 basename.
        // Keep the simfile extension and bytes; only the isolated staging name changes.
        let filename = if cfg!(target_os = "windows") && !filename.to_string_lossy().is_ascii() {
            std::ffi::OsString::from(format!(
                "chart.{}",
                name.extension().unwrap_or_default().to_string_lossy()
            ))
        } else {
            filename.to_owned()
        };
        let staged = Self {
            path: dir.join(filename),
        };
        fs::write(&staged.path, bytes)
            .map_err(|error| Error::Native(format!("stage chart input: {error}")))?;
        Ok(staged)
    }
}
impl Drop for StagedInput {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        if let Some(dir) = self.path.parent() {
            let _ = fs::remove_dir(dir);
        }
    }
}

fn decode(bytes: &[u8]) -> Result<(Vec<Chart>, Vec<String>), Error> {
    let mut input = Reader::new(bytes);
    if input.take(MAGIC.len())? != MAGIC {
        return Err(Error::Wire("invalid native response magic".into()));
    }
    let version = input.u32()?;
    if version != SCHEMA_VERSION {
        return Err(Error::Wire(format!(
            "unsupported native wire version {version}"
        )));
    }
    match input.byte()? {
        0 => {}
        1 => return Err(Error::Native(input.string()?)),
        2 => return Err(Error::Unavailable),
        status => return Err(Error::Wire(format!("invalid native status {status}"))),
    }

    let chart_count = input.len()?;
    let mut charts = Vec::with_capacity(chart_count);
    for _ in 0..chart_count {
        charts.push(read_chart(&mut input)?);
    }
    let diagnostics = (0..input.len()?)
        .map(|_| input.string())
        .collect::<Result<Vec<_>, _>>()?;
    if !input.is_empty() {
        return Err(Error::Wire("native response has trailing bytes".into()));
    }
    Ok((charts, diagnostics))
}

fn read_chart(input: &mut Reader<'_>) -> Result<Chart, Error> {
    let source_index = input.u32()?;
    let title = input.text()?;
    let subtitle = input.text()?;
    let artist = input.text()?;
    let title_translit = input.text()?;
    let subtitle_translit = input.text()?;
    let artist_translit = input.text()?;
    let metadata_utf8 = [
        &title,
        &subtitle,
        &artist,
        &title_translit,
        &subtitle_translit,
        &artist_translit,
    ]
    .iter()
    .all(|text| matches!(text, NativeText::Utf8(_)));
    let credit = input.named_string("credit")?;
    let description = input.named_string("description")?;
    let steps_type = input.steps_type()?;
    let difficulty_code = input.i32()?;
    let meter = input.i32()?;
    let bpm = BpmRange {
        actual_min: input.raw_f32()?,
        actual_max: input.raw_f32()?,
        display_min: input.raw_f32()?,
        display_max: input.raw_f32()?,
    };
    let beat0_offset_seconds = input.raw_f32()?;
    let beat0_group_offset_seconds = input.raw_f32()?;
    let segment_count = input.len()?;
    let mut segments = Vec::with_capacity(segment_count);
    for _ in 0..segment_count {
        let segment_type_code = input.i32()?;
        let row = input.i32()?;
        let beat = input.raw_f32()?;
        let value_count = input.len()?;
        let mut values = Vec::with_capacity(value_count);
        for _ in 0..value_count {
            values.push(input.raw_f32()?);
        }
        let label = match input.byte()? {
            0 => None,
            1 => Some(input.string()?),
            flag => return Err(Error::Wire(format!("invalid label flag {flag}"))),
        };
        segments.push(TimingSegment {
            segment_type_code,
            row,
            beat,
            values,
            label,
        });
    }
    let note_data = read_note_data(input)?;
    let player_count = input.len()?;
    let mut player_stats = Vec::with_capacity(player_count);
    for _ in 0..player_count {
        player_stats.push(PlayerStats {
            player_code: input.i32()?,
            radar: input.raw_f32_vec()?,
            tech_counts: input.raw_f32_vec()?,
            notes_per_measure: (0..input.len()?)
                .map(|_| input.i32())
                .collect::<Result<Vec<_>, _>>()?,
            nps_per_measure: input.raw_f32_vec()?,
            peak_nps: input.raw_f32()?,
        });
    }

    let groove_stats_hash = input.string()?;
    let groove_stats_hash_version = input.i32()?;
    let hash_bpms = input.string()?;
    let first_second = input.raw_f32()?;
    let last_second = input.raw_f32()?;
    let note_data_supported = !player_stats.is_empty();
    let first_second = note_data_supported.then_some(first_second);
    let last_second = note_data_supported.then_some(last_second);
    let theme_json = input.string()?;
    let theme = if theme_json.is_empty() {
        None
    } else {
        Some(
            serde_json::from_str(&theme_json)
                .map_err(|error| Error::Wire(format!("invalid theme response: {error}")))?,
        )
    };
    Ok(Chart {
        source_index,
        title,
        subtitle,
        artist,
        title_translit,
        subtitle_translit,
        artist_translit,
        metadata_utf8,
        credit,
        description,
        steps_type,
        difficulty_code,
        meter,
        bpm,
        timing: Timing {
            beat0_offset_seconds,
            beat0_group_offset_seconds,
            segments,
        },
        note_data,
        note_data_supported,
        player_stats,
        groove_stats_hash,
        groove_stats_hash_version,
        hash_bpms,
        first_second,
        last_second,
        theme,
    })
}

fn read_note_data(input: &mut Reader<'_>) -> Result<NoteData, Error> {
    let tracks = input.u32()?;
    let note_count = input.len()?;
    let mut entries = Vec::with_capacity(note_count);
    for _ in 0..note_count {
        entries.push(Note {
            row: input.i32()?,
            beat: input.raw_f32()?,
            track: input.i32()?,
            note_type_code: input.i32()?,
            sub_type_code: input.i32()?,
            source_code: input.i32()?,
            player_code: input.i32()?,
            duration_rows: input.i32()?,
            duration_beats: input.raw_f32()?,
            attack_duration_seconds: input.raw_f32()?,
            keysound_index: input.i32()?,
            attack_modifiers: input.string()?,
        });
    }
    Ok(NoteData { tracks, entries })
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
        let bytes: [u8; 4] = self.take(4)?.try_into().unwrap();
        Ok(u32::from_le_bytes(bytes))
    }

    fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }

    fn f32(&mut self) -> Result<f32, Error> {
        Ok(f32::from_bits(self.u32()?))
    }

    fn raw_f32(&mut self) -> Result<RawF32, Error> {
        Ok(RawF32(self.f32()?))
    }

    fn raw_f32_vec(&mut self) -> Result<Vec<RawF32>, Error> {
        let len = self.len()?;
        let mut values = Vec::with_capacity(len);
        for _ in 0..len {
            values.push(self.raw_f32()?);
        }
        Ok(values)
    }

    fn len(&mut self) -> Result<usize, Error> {
        usize::try_from(self.u32()?)
            .map_err(|_| Error::Wire("native length does not fit usize".into()))
    }

    fn string(&mut self) -> Result<String, Error> {
        let len = self.len()?;
        String::from_utf8(self.take(len)?.to_vec())
            .map_err(|_| Error::Wire("native response contains invalid UTF-8".into()))
    }

    fn named_string(&mut self, field: &str) -> Result<String, Error> {
        self.string()
            .map_err(|error| Error::Wire(format!("{field}: {error}")))
    }

    fn text(&mut self) -> Result<NativeText, Error> {
        let len = self.len()?;
        let bytes = self.take(len)?;
        Ok(match std::str::from_utf8(bytes) {
            Ok(text) => NativeText::Utf8(text.to_owned()),
            Err(_) => NativeText::Bytes {
                raw_bytes_hex: bytes.iter().map(|b| format!("{b:02x}")).collect(),
            },
        })
    }

    fn steps_type(&mut self) -> Result<StepsType, Error> {
        Ok(StepsType {
            code: self.i32()?,
            source: self.string()?,
        })
    }
}

#[cfg(itgmania_oracle)]
fn native_load(path: &Path, theme: Option<&Path>) -> Result<(Vec<Chart>, Vec<String>), Error> {
    #[repr(C)]
    struct NativeBuffer {
        data: *mut u8,
        len: usize,
    }

    unsafe extern "C" {
        fn itg_oracle_load_charts(
            path: *const u8,
            path_len: usize,
            theme: *const u8,
            theme_len: usize,
            host: *const u8,
            host_len: usize,
        ) -> NativeBuffer;
        fn itg_oracle_free(data: *mut u8);
    }

    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str().as_bytes()
    };
    #[cfg(windows)]
    let encoded = path.to_string_lossy();
    #[cfg(windows)]
    let path = encoded.as_bytes();
    let theme = theme
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default();
    let host = include_bytes!("../chart_theme.lua");
    // SAFETY: all three byte slices remain alive for the call with valid lengths.
    let native = unsafe {
        itg_oracle_load_charts(
            path.as_ptr(),
            path.len(),
            theme.as_ptr(),
            theme.len(),
            host.as_ptr(),
            host.len(),
        )
    };
    if native.data.is_null() {
        return Err(Error::Native("native oracle returned no response".into()));
    }
    // SAFETY: the bridge returns `len` initialized bytes owned by `data` and
    // keeps them valid until the matching free call below.
    let bytes = unsafe { std::slice::from_raw_parts(native.data, native.len) }.to_vec();
    // SAFETY: `data` came from the bridge and has not previously been freed.
    unsafe { itg_oracle_free(native.data) };
    decode(&bytes)
}

#[cfg(not(itgmania_oracle))]
fn native_load(_path: &Path, _theme: Option<&Path>) -> Result<(Vec<Chart>, Vec<String>), Error> {
    let mut response = MAGIC.to_vec();
    response.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
    response.push(2);
    decode(&response)
}

#[derive(Debug)]
pub enum Error {
    Native(String),
    Wire(String),
    Unavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(message) => write!(formatter, "ITGmania oracle failed: {message}"),
            Self::Wire(message) => write!(formatter, "invalid native oracle response: {message}"),
            Self::Unavailable => write!(
                formatter,
                "the native ITGmania oracle is supported on Windows and Linux/WSL only"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_truncated_response() {
        assert!(matches!(decode(MAGIC), Err(Error::Wire(_))));
    }

    #[test]
    fn returns_native_error() {
        let mut bytes = response_header(1);
        push_string(&mut bytes, "loader failed");
        assert!(
            matches!(decode(&bytes), Err(Error::Native(message)) if message == "loader failed")
        );
    }

    #[test]
    fn preserves_float_bits() {
        let mut bytes = response_header(0);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 0);
        for value in ["title", "", "artist", "", "", "", "credit", "chart"] {
            push_string(&mut bytes, value);
        }
        push_steps_type(&mut bytes, 0, "dance-single");
        push_u32(&mut bytes, 4);
        push_u32(&mut bytes, 12);
        for value in [
            f32::from_bits(0x3f80_0001),
            200.0,
            -0.0,
            200.0,
            f32::from_bits(0xbdcc_cccd),
            0.0,
        ] {
            push_u32(&mut bytes, value.to_bits());
        }
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 4);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);

        push_string(&mut bytes, "0123456789abcdef");
        push_u32(&mut bytes, 3);
        push_string(&mut bytes, "0.000=150.000");
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_string(&mut bytes, "");
        push_u32(&mut bytes, 0);
        let (charts, _) = decode(&bytes).unwrap();
        assert_eq!(charts[0].bpm.actual_min.0.to_bits(), 0x3f80_0001);
        assert_eq!(charts[0].bpm.display_min.0.to_bits(), (-0.0_f32).to_bits());
        assert_eq!(
            charts[0].timing.beat0_offset_seconds.0.to_bits(),
            0xbdcc_cccd
        );
    }

    #[test]
    fn serializes_non_finite_bits_explicitly() {
        let json = serde_json::to_string(&RawF32(f32::from_bits(0x7fc0_0042))).unwrap();
        assert_eq!(json, r#"{"non_finite":"nan","bits":"0x7fc00042"}"#);
    }

    #[test]
    fn does_not_force_decimal_precision() {
        assert_eq!(
            serde_json::to_string(&RawF32(f32::from_bits(0x3f80_0001))).unwrap(),
            "1.0000001"
        );
        assert_eq!(serde_json::to_string(&RawF32(-0.0)).unwrap(), "-0.0");
    }

    fn response_header(status: u8) -> Vec<u8> {
        let mut bytes = MAGIC.to_vec();
        push_u32(&mut bytes, SCHEMA_VERSION);
        bytes.push(status);
        bytes
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_string(bytes: &mut Vec<u8>, value: &str) {
        push_u32(bytes, value.len() as u32);
        bytes.extend_from_slice(value.as_bytes());
    }

    fn push_steps_type(bytes: &mut Vec<u8>, code: i32, name: &str) {
        push_u32(bytes, code as u32);
        push_string(bytes, name);
    }
}
