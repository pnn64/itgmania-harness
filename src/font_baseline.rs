use crate::font_oracle;
use crate::oracle::RawF32;
use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub struct Report {
    pub fonts: usize,
    pub diagnostics: usize,
}

#[derive(Serialize)]
struct Fixture {
    schema_version: u32,
    oracle_schema_version: u32,
    font: String,
    corpus: &'static str,
    height: i32,
    line_spacing: i32,
    right_to_left: bool,
    distance_field: bool,
    default_stroke_color: [RawF32; 4],
    line_width: i32,
    line_height: i32,
    pages: Vec<Page>,
    glyphs: Vec<Glyph>,
}

#[derive(Serialize)]
struct Page {
    texture: String,
    texture_hints: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    stroke_texture: Option<String>,
    source_size: Size,
    image_size: Size,
    frames: Size,
    height: i32,
    line_spacing: i32,
    vertical_shift: RawF32,
    draw_extra_pixels_left: i32,
    draw_extra_pixels_right: i32,
}

#[derive(Clone, Copy, Serialize)]
struct Size {
    width: i32,
    height: i32,
}

#[derive(Serialize)]
struct Glyph {
    codepoint: u32,
    uses_default_glyph: bool,
    texture: String,
    frame: u32,
    pen_x: i32,
    advance: i32,
    width: RawF32,
    height: RawF32,
    horizontal_shift: RawF32,
    vertical_shift: RawF32,
    sample_rect: Rect,
}

#[derive(Serialize)]
struct Rect {
    left: RawF32,
    top: RawF32,
    right: RawF32,
    bottom: RawF32,
}

#[derive(Serialize)]
struct Manifest<'a> {
    fixture_schema_version: u32,
    oracle_schema_version: u32,
    harness_version: &'a str,
    corpus: &'static str,
    itgmania: Itgmania<'a>,
    fonts: Vec<ManifestFont>,
}

#[derive(Serialize)]
struct Itgmania<'a> {
    git_revision: &'a str,
    git_dirty: &'a str,
}

#[derive(Serialize)]
struct ManifestFont {
    font: String,
    fixture: String,
    glyphs: usize,
}

struct PageInfo {
    texture: String,
    texture_size: Size,
}

pub fn generate(font_root: &Path, output_dir: &Path) -> Result<Report, Error> {
    let font_root = font_root
        .canonicalize()
        .map_err(|source| Error::io("open font root", font_root, source))?;
    let mut font_paths = Vec::new();
    collect_fonts(&font_root, &mut font_paths)?;
    font_paths.sort();
    if font_paths.is_empty() {
        return Err(Error::NoFonts(font_root));
    }

    let mut manifest_fonts = Vec::with_capacity(font_paths.len());
    let mut diagnostic_count = 0;
    for font_path in font_paths {
        let mapped = font_oracle::load_mapped(&font_path).map_err(|source| Error::Oracle {
            path: font_path.clone(),
            source,
        })?;
        let corpus = mapped_corpus(&font_path, &mapped)?;
        let document = font_oracle::load(&font_path, &corpus).map_err(|source| Error::Oracle {
            path: font_path.clone(),
            source,
        })?;
        diagnostic_count += document.diagnostics.len();
        let fixture = portable_fixture(&font_root, &font_path, document)?;
        let relative_font = relative_path(&font_root, &font_path)?;
        let relative_fixture = Path::new(&relative_font).with_extension("json");
        write_json(&output_dir.join(&relative_fixture), &fixture)?;
        manifest_fonts.push(ManifestFont {
            font: relative_font,
            fixture: slash_path(&relative_fixture),
            glyphs: fixture.glyphs.len(),
        });
    }

    let manifest = Manifest {
        fixture_schema_version: FIXTURE_SCHEMA_VERSION,
        oracle_schema_version: font_oracle::SCHEMA_VERSION,
        harness_version: env!("CARGO_PKG_VERSION"),
        corpus: "all-complete-bmp-plus-default-and-missing",
        itgmania: Itgmania {
            git_revision: env!("ITGMANIA_GIT_REVISION"),
            git_dirty: env!("ITGMANIA_GIT_DIRTY"),
        },
        fonts: manifest_fonts,
    };
    write_json(&output_dir.join("_manifest.json"), &manifest)?;
    Ok(Report {
        fonts: manifest.fonts.len(),
        diagnostics: diagnostic_count,
    })
}

fn mapped_corpus(path: &Path, mapped: &font_oracle::Document) -> Result<String, Error> {
    let source =
        fs::read_to_string(path).map_err(|error| Error::io("read font ini", path, error))?;
    let astral = source
        .lines()
        .filter_map(|line| line.split_once('='))
        .filter(|(key, _)| key.trim().to_ascii_lowercase().starts_with("line "))
        .flat_map(|(_, value)| value.chars())
        .filter(|character| *character as u32 > 0xFFFF)
        .collect::<BTreeSet<_>>();
    let truncated = astral
        .iter()
        .map(|character| *character as u32 & 0xFFFF)
        .collect::<BTreeSet<_>>();

    let mut text = String::with_capacity(mapped.glyphs.len() + astral.len() * 4);
    for glyph in &mapped.glyphs {
        if !glyph.uses_default_glyph && !truncated.contains(&glyph.codepoint) {
            let character =
                char::from_u32(glyph.codepoint).ok_or(Error::InvalidCodepoint(glyph.codepoint))?;
            text.push(character);
        }
    }
    text.extend(astral);
    text.push('\u{F8FF}');
    let missing = mapped
        .glyphs
        .iter()
        .find(|glyph| glyph.uses_default_glyph && glyph.codepoint != 0xF8FF)
        .map_or(0x0378, |glyph| glyph.codepoint);
    text.push(char::from_u32(missing).ok_or(Error::InvalidCodepoint(missing))?);
    Ok(text)
}

fn portable_fixture(
    root: &Path,
    font_path: &Path,
    document: font_oracle::Document,
) -> Result<Fixture, Error> {
    let mut page_info = Vec::with_capacity(document.observed_pages.len());
    let mut pages = Vec::with_capacity(document.observed_pages.len());
    for page in document.observed_pages {
        let texture = relative_string(root, &page.texture)?;
        let texture_size = Size {
            width: page.texture_size.width,
            height: page.texture_size.height,
        };
        page_info.push(PageInfo {
            texture: texture.clone(),
            texture_size,
        });
        pages.push(Page {
            texture,
            texture_hints: page.texture_hints,
            stroke_texture: page
                .stroke_texture
                .as_deref()
                .map(|path| relative_string(root, path))
                .transpose()?,
            source_size: Size {
                width: page.source_size.width,
                height: page.source_size.height,
            },
            image_size: Size {
                width: page.image_size.width,
                height: page.image_size.height,
            },
            frames: Size {
                width: page.frames.width,
                height: page.frames.height,
            },
            height: page.height,
            line_spacing: page.line_spacing,
            vertical_shift: page.vertical_shift,
            draw_extra_pixels_left: page.draw_extra_pixels_left,
            draw_extra_pixels_right: page.draw_extra_pixels_right,
        });
    }

    let mut glyphs = Vec::with_capacity(document.glyphs.len());
    for glyph in document.glyphs {
        let page = page_info
            .get(glyph.page as usize)
            .ok_or(Error::InvalidPage(glyph.page))?;
        let scale_x = page.texture_size.width as f32;
        let scale_y = page.texture_size.height as f32;
        glyphs.push(Glyph {
            codepoint: glyph.codepoint,
            uses_default_glyph: glyph.uses_default_glyph,
            texture: page.texture.clone(),
            frame: glyph.frame,
            pen_x: glyph.pen_x,
            advance: glyph.advance,
            width: glyph.width,
            height: glyph.height,
            horizontal_shift: glyph.horizontal_shift,
            vertical_shift: glyph.vertical_shift,
            sample_rect: Rect {
                left: scaled(glyph.texture_rect.left, scale_x),
                top: scaled(glyph.texture_rect.top, scale_y),
                right: scaled(glyph.texture_rect.right, scale_x),
                bottom: scaled(glyph.texture_rect.bottom, scale_y),
            },
        });
    }

    Ok(Fixture {
        schema_version: FIXTURE_SCHEMA_VERSION,
        oracle_schema_version: font_oracle::SCHEMA_VERSION,
        font: relative_path(root, font_path)?,
        corpus: "all-complete-bmp-plus-default-and-missing",
        height: document.height,
        line_spacing: document.line_spacing,
        right_to_left: document.right_to_left,
        distance_field: document.distance_field,
        default_stroke_color: document.default_stroke_color,
        line_width: document.line_width,
        line_height: document.line_height,
        pages,
        glyphs,
    })
}

fn scaled(value: RawF32, scale: f32) -> RawF32 {
    RawF32::from_bits((value.get() * scale).to_bits())
}

fn collect_fonts(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), Error> {
    let entries = fs::read_dir(directory)
        .map_err(|source| Error::io("read font directory", directory, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::io("read font directory", directory, source))?;
        let path = entry.path();
        if path.is_dir() {
            collect_fonts(&path, output)?;
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("ini"))
        {
            output.push(path);
        }
    }
    Ok(())
}

fn relative_string(root: &Path, path: &str) -> Result<String, Error> {
    let path = Path::new(path)
        .canonicalize()
        .map_err(|source| Error::io("open font resource", Path::new(path), source))?;
    relative_path(root, &path)
}

fn relative_path(root: &Path, path: &Path) -> Result<String, Error> {
    path.strip_prefix(root)
        .map(slash_path)
        .map_err(|_| Error::OutsideRoot {
            root: root.to_owned(),
            path: path.to_owned(),
        })
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|source| Error::io("create fixture directory", parent, source))?;
    let mut bytes = serde_json::to_vec_pretty(value).map_err(Error::Serialize)?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|source| Error::io("write font fixture", path, source))
}

#[derive(Debug)]
pub enum Error {
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    NoFonts(PathBuf),
    Oracle {
        path: PathBuf,
        source: font_oracle::Error,
    },
    OutsideRoot {
        root: PathBuf,
        path: PathBuf,
    },
    InvalidPage(u32),
    InvalidCodepoint(u32),
    Serialize(serde_json::Error),
}

impl Error {
    fn io(action: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_owned(),
            source,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "could not {action} {}: {source}", path.display()),
            Self::NoFonts(path) => write!(
                formatter,
                "no font .ini files found under {}",
                path.display()
            ),
            Self::Oracle { path, source } => {
                write!(
                    formatter,
                    "font fixture {} failed: {source}",
                    path.display()
                )
            }
            Self::OutsideRoot { root, path } => write!(
                formatter,
                "font resource {} is outside fixture root {}",
                path.display(),
                root.display()
            ),
            Self::InvalidPage(page) => {
                write!(formatter, "font oracle returned invalid page {page}")
            }
            Self::InvalidCodepoint(codepoint) => {
                write!(
                    formatter,
                    "font oracle returned invalid codepoint U+{codepoint:04X}"
                )
            }
            Self::Serialize(error) => {
                write!(formatter, "could not serialize font fixture: {error}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_paths_are_portable() {
        assert_eq!(slash_path(Path::new("miso\\font.ini")), "miso/font.ini");
    }
}
