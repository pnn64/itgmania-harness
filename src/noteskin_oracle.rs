use crate::oracle::RawF32;
use serde::Serialize;
use std::fmt;
use std::path::Path;

const MAGIC: &[u8; 8] = b"ITGNSKN\0";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct MetricQuery {
    pub section: String,
    pub key: String,
}

#[derive(Clone, Debug)]
pub struct PathQuery {
    pub button: String,
    pub element: String,
}

#[derive(Debug, Serialize)]
pub struct Document {
    pub schema_version: u32,
    pub noteskins_root: String,
    pub game: String,
    pub skin: String,
    pub inventory: Vec<String>,
    pub metrics: Vec<Metric>,
    pub paths: Vec<ResolvedPath>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Metric {
    pub section: String,
    pub key: String,
    pub raw: String,
    pub integer: i32,
    pub float: RawF32,
    pub boolean: bool,
}

#[derive(Debug, Serialize)]
pub struct ResolvedPath {
    pub button: String,
    pub element: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

pub fn load(
    root: &Path,
    game: &str,
    skin: &str,
    metrics: &[MetricQuery],
    paths: &[PathQuery],
) -> Result<Document, Error> {
    let request = encode_request(metrics, paths)?;
    decode(&native_load(root, game, skin, &request)?)
}

fn encode_request(metrics: &[MetricQuery], paths: &[PathQuery]) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    write_len(&mut out, metrics.len(), "metric query count")?;
    for metric in metrics {
        write_string(&mut out, &metric.section)?;
        write_string(&mut out, &metric.key)?;
    }
    write_len(&mut out, paths.len(), "path query count")?;
    for path in paths {
        write_string(&mut out, &path.button)?;
        write_string(&mut out, &path.element)?;
    }
    Ok(out)
}

fn write_len(out: &mut Vec<u8>, value: usize, field: &str) -> Result<(), Error> {
    let value = u32::try_from(value)
        .map_err(|_| Error::Request(format!("{field} exceeds native wire limit")))?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_string(out: &mut Vec<u8>, value: &str) -> Result<(), Error> {
    write_len(out, value.len(), "request string")?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn decode(bytes: &[u8]) -> Result<Document, Error> {
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
        status => return Err(Error::Wire(format!("invalid native status {status}"))),
    }

    let noteskins_root = input.string()?;
    let game = input.string()?;
    let skin = input.string()?;
    let inventory = input.strings()?;
    let metric_count = input.len()?;
    let mut metrics = Vec::with_capacity(metric_count);
    for _ in 0..metric_count {
        metrics.push(Metric {
            section: input.string()?,
            key: input.string()?,
            raw: input.string()?,
            integer: input.i32()?,
            float: input.raw_f32()?,
            boolean: input.boolean()?,
        });
    }
    let path_count = input.len()?;
    let mut paths = Vec::with_capacity(path_count);
    for _ in 0..path_count {
        let button = input.string()?;
        let element = input.string()?;
        let path = input.string()?;
        paths.push(ResolvedPath {
            button,
            element,
            path: (!path.is_empty()).then_some(path),
        });
    }
    let diagnostics = input.strings()?;
    if !input.is_empty() {
        return Err(Error::Wire("native response has trailing bytes".into()));
    }
    Ok(Document {
        schema_version: SCHEMA_VERSION,
        noteskins_root,
        game,
        skin,
        inventory,
        metrics,
        paths,
        diagnostics,
    })
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

    fn boolean(&mut self) -> Result<bool, Error> {
        match self.byte()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(Error::Wire(format!("invalid native boolean {value}"))),
        }
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes: [u8; 4] = self.take(4)?.try_into().unwrap();
        Ok(u32::from_le_bytes(bytes))
    }

    fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }

    fn raw_f32(&mut self) -> Result<RawF32, Error> {
        Ok(RawF32::from_bits(self.u32()?))
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

    fn strings(&mut self) -> Result<Vec<String>, Error> {
        let count = self.len()?;
        (0..count).map(|_| self.string()).collect()
    }
}

#[cfg(itgmania_oracle)]
fn native_load(root: &Path, game: &str, skin: &str, request: &[u8]) -> Result<Vec<u8>, Error> {
    #[repr(C)]
    struct NativeBuffer {
        data: *mut u8,
        len: usize,
    }

    unsafe extern "C" {
        fn itg_oracle_load_noteskin(
            root: *const u8,
            root_len: usize,
            game: *const u8,
            game_len: usize,
            skin: *const u8,
            skin_len: usize,
            request: *const u8,
            request_len: usize,
        ) -> NativeBuffer;
        fn itg_oracle_free(data: *mut u8);
    }

    #[cfg(unix)]
    let root = {
        use std::os::unix::ffi::OsStrExt;
        root.as_os_str().as_bytes()
    };
    #[cfg(windows)]
    let encoded = root.to_string_lossy();
    #[cfg(windows)]
    let root = encoded.as_bytes();
    let game = game.as_bytes();
    let skin = skin.as_bytes();
    // SAFETY: all byte slices remain alive for the call and carry explicit lengths.
    let native = unsafe {
        itg_oracle_load_noteskin(
            root.as_ptr(),
            root.len(),
            game.as_ptr(),
            game.len(),
            skin.as_ptr(),
            skin.len(),
            request.as_ptr(),
            request.len(),
        )
    };
    if native.data.is_null() {
        return Err(Error::Native(
            "native noteskin oracle returned no response".into(),
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
fn native_load(_root: &Path, _game: &str, _skin: &str, _request: &[u8]) -> Result<Vec<u8>, Error> {
    Err(Error::Unavailable)
}

#[derive(Debug)]
pub enum Error {
    Request(String),
    Native(String),
    Wire(String),
    #[cfg(not(itgmania_oracle))]
    Unavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Request(message) => write!(formatter, "invalid noteskin request: {message}"),
            Self::Native(message) => {
                write!(formatter, "ITGmania noteskin oracle failed: {message}")
            }
            Self::Wire(message) => write!(formatter, "invalid native noteskin response: {message}"),
            #[cfg(not(itgmania_oracle))]
            Self::Unavailable => write!(
                formatter,
                "the native ITGmania noteskin oracle is supported on Windows and Linux/WSL only"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip_layout_starts_with_metric_count() {
        let request = encode_request(
            &[MetricQuery {
                section: "NoteDisplay".into(),
                key: "TapNoteAnimationLength".into(),
            }],
            &[],
        )
        .unwrap();
        assert_eq!(&request[..4], &1_u32.to_le_bytes());
    }

    #[test]
    fn rejects_truncated_response() {
        assert!(matches!(decode(MAGIC), Err(Error::Wire(_))));
    }
}
