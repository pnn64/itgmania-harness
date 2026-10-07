use crate::oracle::RawF32;
use serde::Serialize;
use std::fmt;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"ITGSLUA\0";
pub const SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Serialize)]
pub struct Document {
    pub schema_version: u32,
    pub simfile: String,
    pub song_dir: String,
    pub title: String,
    pub specified_last_second: RawF32,
    pub specified_last_beat: RawF32,
    pub changes: Vec<Change>,
}

#[derive(Debug, Serialize)]
pub struct Change {
    pub layer: Layer,
    pub index: u32,
    pub start_beat: RawF32,
    pub rate: RawF32,
    pub effect: String,
    pub file1: String,
    pub file2: String,
    pub color1: String,
    pub color2: String,
    pub transition: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lua_entry: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Background1,
    Background2,
    Foreground,
}

pub fn load(path: &Path) -> Result<Document, Error> {
    let mut document = decode(&native_load(path)?)?;
    let song_dir = path.parent().unwrap_or_else(|| Path::new("."));
    for change in &mut document.changes {
        change.lua_entry = resolve_lua_entry(song_dir, &change.file1)
            .map(|entry| entry.to_string_lossy().into_owned());
    }
    Ok(document)
}

fn resolve_lua_entry(song_dir: &Path, file: &str) -> Option<PathBuf> {
    if file.is_empty() {
        return None;
    }
    let path = song_dir.join(file.replace('\\', "/"));
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lua"))
    {
        return Some(path);
    }
    let default = path.join("default.lua");
    default.is_file().then_some(default)
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

    let simfile = input.string()?;
    let song_dir = input.string()?;
    let title = input.string()?;
    let specified_last_second = input.raw_f32()?;
    let specified_last_beat = input.raw_f32()?;
    let count = input.len()?;
    let mut changes = Vec::with_capacity(count);
    for _ in 0..count {
        changes.push(Change {
            layer: match input.u32()? {
                0 => Layer::Background1,
                1 => Layer::Background2,
                2 => Layer::Foreground,
                layer => return Err(Error::Wire(format!("invalid song Lua layer {layer}"))),
            },
            index: input.u32()?,
            start_beat: input.raw_f32()?,
            rate: input.raw_f32()?,
            effect: input.string()?,
            file1: input.string()?,
            file2: input.string()?,
            color1: input.string()?,
            color2: input.string()?,
            transition: input.string()?,
            lua_entry: None,
        });
    }
    if !input.is_empty() {
        return Err(Error::Wire("native response has trailing bytes".into()));
    }
    Ok(Document {
        schema_version: SCHEMA_VERSION,
        simfile,
        song_dir,
        title,
        specified_last_second,
        specified_last_beat,
        changes,
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

    fn u32(&mut self) -> Result<u32, Error> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| Error::Wire("truncated native integer".into()))?;
        Ok(u32::from_le_bytes(bytes))
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
}

#[cfg(itgmania_oracle)]
fn native_load(path: &Path) -> Result<Vec<u8>, Error> {
    #[repr(C)]
    struct NativeBuffer {
        data: *mut u8,
        len: usize,
    }

    unsafe extern "C" {
        fn itg_oracle_load_song_lua(path: *const u8, path_len: usize) -> NativeBuffer;
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
    // SAFETY: the path byte slice remains alive and carries its explicit length.
    let native = unsafe { itg_oracle_load_song_lua(path.as_ptr(), path.len()) };
    if native.data.is_null() {
        return Err(Error::Native(
            "native song Lua oracle returned no response".into(),
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
fn native_load(_path: &Path) -> Result<Vec<u8>, Error> {
    Err(Error::Unavailable)
}

#[derive(Debug)]
pub enum Error {
    Native(String),
    Wire(String),
    #[cfg(not(itgmania_oracle))]
    Unavailable,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native(message) => {
                write!(formatter, "ITGmania song Lua oracle failed: {message}")
            }
            Self::Wire(message) => write!(formatter, "invalid native song Lua response: {message}"),
            #[cfg(not(itgmania_oracle))]
            Self::Unavailable => write!(
                formatter,
                "the native ITGmania song Lua oracle is supported on Windows and Linux/WSL only"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_lua_files_case_insensitively() {
        assert_eq!(
            resolve_lua_entry(Path::new("song"), "fg/Default.LUA"),
            Some(PathBuf::from("song/fg/Default.LUA"))
        );
        assert_eq!(resolve_lua_entry(Path::new("song"), "movie.mp4"), None);
    }

    #[test]
    fn rejects_truncated_response() {
        assert!(matches!(decode(MAGIC), Err(Error::Wire(_))));
    }
}
