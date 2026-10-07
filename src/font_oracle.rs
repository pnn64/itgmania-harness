use crate::oracle::RawF32;
use serde::Serialize;
use std::fmt;
use std::path::Path;

const MAGIC: &[u8; 8] = b"ITGFONT\0";
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct Document {
    pub schema_version: u32,
    pub font: String,
    pub theme_fonts: String,
    pub fallback_fonts: String,
    pub texture_backend: String,
    pub text: String,
    pub height: i32,
    pub line_spacing: i32,
    pub right_to_left: bool,
    pub distance_field: bool,
    pub default_stroke_color: [RawF32; 4],
    pub line_width: i32,
    pub line_height: i32,
    pub observed_pages: Vec<Page>,
    pub glyphs: Vec<Glyph>,
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Page {
    pub texture: String,
    pub texture_hints: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke_texture: Option<String>,
    pub source_size: Size,
    pub image_size: Size,
    pub texture_size: Size,
    pub frames: Size,
    pub height: i32,
    pub line_spacing: i32,
    pub vertical_shift: RawF32,
    pub draw_extra_pixels_left: i32,
    pub draw_extra_pixels_right: i32,
}

#[derive(Debug, Serialize)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Serialize)]
pub struct Glyph {
    pub text_index: u32,
    pub codepoint: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character: Option<String>,
    pub uses_default_glyph: bool,
    pub page: u32,
    pub frame: u32,
    pub pen_x: i32,
    pub advance: i32,
    pub width: RawF32,
    pub height: RawF32,
    pub horizontal_shift: RawF32,
    pub vertical_shift: RawF32,
    pub texture_rect: Rect,
}

#[derive(Debug, Serialize)]
pub struct Rect {
    pub left: RawF32,
    pub top: RawF32,
    pub right: RawF32,
    pub bottom: RawF32,
}

pub fn load(path: &Path, text: &str) -> Result<Document, Error> {
    let mut document = decode(&native_load(path, text, false)?)?;
    let mut characters = text.chars();
    for glyph in &mut document.glyphs {
        let character = characters.next().ok_or_else(|| {
            Error::Wire("native font response has more glyphs than requested text".into())
        })?;
        glyph.codepoint = character as u32;
        glyph.character = Some(character.to_string());
    }
    if characters.next().is_some() {
        return Err(Error::Wire(
            "native font response has fewer glyphs than requested text".into(),
        ));
    }
    document.text = text.to_owned();
    Ok(document)
}

pub fn load_mapped(path: &Path) -> Result<Document, Error> {
    decode(&native_load(path, "", true)?)
}

fn decode(bytes: &[u8]) -> Result<Document, Error> {
    let mut input = Reader::new(bytes);
    if input.take(MAGIC.len())? != MAGIC {
        return Err(Error::Wire("invalid native font response magic".into()));
    }
    let version = input.u32()?;
    if version != SCHEMA_VERSION {
        return Err(Error::Wire(format!(
            "unsupported native font wire version {version}"
        )));
    }
    match input.byte()? {
        0 => {}
        1 => return Err(Error::Native(input.string()?)),
        2 => return Err(Error::Unavailable),
        status => return Err(Error::Wire(format!("invalid native font status {status}"))),
    }

    let font = input.string()?;
    let theme_fonts = input.string()?;
    let fallback_fonts = input.string()?;
    let texture_backend = input.string()?;
    let text = input.string()?;
    let height = input.i32()?;
    let line_spacing = input.i32()?;
    let right_to_left = input.boolean()?;
    let distance_field = input.boolean()?;
    let default_stroke_color = [
        input.raw_f32()?,
        input.raw_f32()?,
        input.raw_f32()?,
        input.raw_f32()?,
    ];
    let line_width = input.i32()?;
    let line_height = input.i32()?;
    let page_count = input.len()?;
    let mut observed_pages = Vec::with_capacity(page_count);
    for _ in 0..page_count {
        observed_pages.push(read_page(&mut input)?);
    }
    let glyph_count = input.len()?;
    let mut glyphs = Vec::with_capacity(glyph_count);
    for text_index in 0..glyph_count {
        glyphs.push(read_glyph(&mut input, text_index)?);
    }
    let diagnostic_count = input.len()?;
    let mut diagnostics = Vec::with_capacity(diagnostic_count);
    for _ in 0..diagnostic_count {
        diagnostics.push(input.string()?);
    }
    if !input.is_empty() {
        return Err(Error::Wire(
            "native font response has trailing bytes".into(),
        ));
    }

    Ok(Document {
        schema_version: SCHEMA_VERSION,
        font,
        theme_fonts,
        fallback_fonts,
        texture_backend,
        text,
        height,
        line_spacing,
        right_to_left,
        distance_field,
        default_stroke_color,
        line_width,
        line_height,
        observed_pages,
        glyphs,
        diagnostics,
    })
}

fn read_page(input: &mut Reader<'_>) -> Result<Page, Error> {
    let texture = input.string()?;
    let texture_hints = input.string()?;
    let stroke_texture = if input.boolean()? {
        Some(input.string()?)
    } else {
        None
    };
    Ok(Page {
        texture,
        texture_hints,
        stroke_texture,
        source_size: input.size()?,
        image_size: input.size()?,
        texture_size: input.size()?,
        frames: input.size()?,
        height: input.i32()?,
        line_spacing: input.i32()?,
        vertical_shift: input.raw_f32()?,
        draw_extra_pixels_left: input.i32()?,
        draw_extra_pixels_right: input.i32()?,
    })
}

fn read_glyph(input: &mut Reader<'_>, text_index: usize) -> Result<Glyph, Error> {
    let codepoint = input.u32()?;
    let available = input.boolean()?;
    Ok(Glyph {
        text_index: u32::try_from(text_index)
            .map_err(|_| Error::Wire("font text index does not fit u32".into()))?,
        codepoint,
        character: char::from_u32(codepoint).map(|value| value.to_string()),
        uses_default_glyph: !available,
        page: input.u32()?,
        frame: input.u32()?,
        pen_x: input.i32()?,
        advance: input.i32()?,
        width: input.raw_f32()?,
        height: input.raw_f32()?,
        horizontal_shift: input.raw_f32()?,
        vertical_shift: input.raw_f32()?,
        texture_rect: Rect {
            left: input.raw_f32()?,
            top: input.raw_f32()?,
            right: input.raw_f32()?,
            bottom: input.raw_f32()?,
        },
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
            .ok_or_else(|| Error::Wire("native font response offset overflow".into()))?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| Error::Wire("truncated native font response".into()))?;
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
            .map_err(|_| Error::Wire("native font length does not fit usize".into()))
    }

    fn string(&mut self) -> Result<String, Error> {
        let len = self.len()?;
        String::from_utf8(self.take(len)?.to_vec())
            .map_err(|_| Error::Wire("native font response contains invalid UTF-8".into()))
    }

    fn size(&mut self) -> Result<Size, Error> {
        Ok(Size {
            width: self.i32()?,
            height: self.i32()?,
        })
    }
}

#[cfg(itgmania_oracle)]
fn native_load(path: &Path, text: &str, mapped_only: bool) -> Result<Vec<u8>, Error> {
    #[repr(C)]
    struct NativeBuffer {
        data: *mut u8,
        len: usize,
    }

    unsafe extern "C" {
        fn itg_oracle_load_font(
            path: *const u8,
            path_len: usize,
            text: *const u8,
            text_len: usize,
            mapped_only: u8,
            source_root: *const u8,
            source_root_len: usize,
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
    let text = text.as_bytes();
    let source_root = env!("ITGMANIA_BUILD_ROOT").as_bytes();
    // SAFETY: all byte slices remain alive for the call and carry explicit lengths.
    let native = unsafe {
        itg_oracle_load_font(
            path.as_ptr(),
            path.len(),
            text.as_ptr(),
            text.len(),
            u8::from(mapped_only),
            source_root.as_ptr(),
            source_root.len(),
        )
    };
    if native.data.is_null() {
        return Err(Error::Native(
            "native font oracle returned no response".into(),
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
fn native_load(_path: &Path, _text: &str, _mapped_only: bool) -> Result<Vec<u8>, Error> {
    Err(Error::Unavailable)
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
            Self::Native(message) => write!(formatter, "ITGmania font oracle failed: {message}"),
            Self::Wire(message) => write!(formatter, "invalid native font response: {message}"),
            Self::Unavailable => write!(
                formatter,
                "the native ITGmania font oracle is supported on Windows and Linux/WSL only"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_truncated_font_response() {
        assert!(matches!(decode(MAGIC), Err(Error::Wire(_))));
    }

    #[test]
    fn returns_native_font_error() {
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
        bytes.push(1);
        bytes.extend_from_slice(&6_u32.to_le_bytes());
        bytes.extend_from_slice(b"failed");
        assert!(matches!(
            decode(&bytes),
            Err(Error::Native(message)) if message == "failed"
        ));
    }
}
