use crate::noteskin_oracle::{self, MetricQuery, PathQuery};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_SCHEMA_VERSION: u32 = 1;
const GAMES: &[(&str, &[&str])] = &[
    (
        "dance",
        &["Left", "Right", "Up", "Down", "UpLeft", "UpRight"],
    ),
    (
        "pump",
        &["UpLeft", "UpRight", "Center", "DownLeft", "DownRight"],
    ),
    (
        "techno",
        &[
            "Left",
            "Right",
            "Up",
            "Down",
            "UpLeft",
            "UpRight",
            "Center",
            "DownLeft",
            "DownRight",
        ],
    ),
    (
        "lights",
        &[
            "MarqueeUpLeft",
            "MarqueeUpRight",
            "MarqueeLrLeft",
            "MarqueeLrRight",
            "ButtonsLeft",
            "ButtonsRight",
            "BassLeft",
            "BassRight",
        ],
    ),
];
const CORE_ELEMENTS: &[&str] = &[
    "Explosion",
    "Go Receptor",
    "HitMine Explosion",
    "Hold Body Active",
    "Hold Body Inactive",
    "Hold BottomCap Active",
    "Hold BottomCap Inactive",
    "Hold Explosion",
    "Hold Head Active",
    "Hold Head Inactive",
    "Hold Tail Active",
    "Hold Tail Inactive",
    "Hold TopCap Active",
    "Hold TopCap Inactive",
    "Ready Receptor",
    "Receptor",
    "Roll Body Active",
    "Roll Body Inactive",
    "Roll BottomCap Active",
    "Roll BottomCap Inactive",
    "Roll Explosion",
    "Roll Head Active",
    "Roll Head Inactive",
    "Roll Tail Active",
    "Roll Tail Inactive",
    "Roll TopCap Active",
    "Roll TopCap Inactive",
    "Tap Explosion Bright",
    "Tap Explosion Dim",
    "Tap Fake",
    "Tap Lift",
    "Tap Mine",
    "Tap Note",
];

#[derive(Debug)]
pub struct Report {
    pub skins: usize,
    pub metrics: usize,
    pub paths: usize,
    pub diagnostics: usize,
}

#[derive(Serialize)]
struct Fixture {
    schema_version: u32,
    oracle_schema_version: u32,
    game: String,
    skin: String,
    metrics: Vec<noteskin_oracle::Metric>,
    paths: Vec<noteskin_oracle::ResolvedPath>,
}

#[derive(Serialize)]
struct Manifest<'a> {
    fixture_schema_version: u32,
    oracle_schema_version: u32,
    harness_version: &'a str,
    corpus: Corpus,
    itgmania: Itgmania<'a>,
    games: Vec<ManifestGame>,
    fixtures: Vec<ManifestFixture>,
    source_files: Vec<SourceFile>,
}

#[derive(Serialize)]
struct Corpus {
    metrics: &'static str,
    paths: &'static str,
    source_snapshot: &'static str,
}

#[derive(Serialize)]
struct Itgmania<'a> {
    git_revision: &'a str,
    git_dirty: &'a str,
}

#[derive(Serialize)]
struct ManifestGame {
    game: String,
    buttons: Vec<String>,
    skins: Vec<String>,
}

#[derive(Serialize)]
struct ManifestFixture {
    game: String,
    skin: String,
    fixture: String,
    metrics: usize,
    paths: usize,
}

#[derive(Serialize)]
struct SourceFile {
    path: String,
    bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

pub fn probe(root: &Path, game: &str, skin: &str) -> Result<noteskin_oracle::Document, Error> {
    let root = canonical_root(root)?;
    let metrics = metric_queries(&root)?;
    let buttons = game_buttons(game).ok_or_else(|| Error::UnsupportedGame(game.into()))?;
    let paths = path_queries(&root, game, buttons)?;
    noteskin_oracle::load(&root, game, skin, &metrics, &paths).map_err(|source| Error::Oracle {
        game: game.into(),
        skin: skin.into(),
        source,
    })
}

pub fn generate(root: &Path, output_dir: &Path) -> Result<Report, Error> {
    let root = canonical_root(root)?;
    let metrics = metric_queries(&root)?;
    let source_files = source_snapshot(&root)?;
    let mut games = Vec::new();
    let mut fixtures = Vec::new();
    let mut diagnostic_count = 0;
    let mut path_count = 0;

    for &(game, buttons) in GAMES {
        let paths = path_queries(&root, game, buttons)?;
        let candidates = skin_dirs(&root, game)?;
        let Some(first) = candidates.first() else {
            continue;
        };
        let first_document =
            noteskin_oracle::load(&root, game, first, &metrics, &paths).map_err(|source| {
                Error::Oracle {
                    game: game.into(),
                    skin: first.clone(),
                    source,
                }
            })?;
        let inventory = first_document.inventory.clone();
        let mut documents = vec![first_document];
        for skin in inventory.iter().filter(|skin| *skin != first) {
            documents.push(
                noteskin_oracle::load(&root, game, skin, &metrics, &paths).map_err(|source| {
                    Error::Oracle {
                        game: game.into(),
                        skin: skin.clone(),
                        source,
                    }
                })?,
            );
        }

        for document in documents {
            diagnostic_count += document.diagnostics.len();
            path_count += document.paths.len();
            let relative_fixture = Path::new(game).join(format!("{}.json", document.skin));
            let fixture = Fixture {
                schema_version: FIXTURE_SCHEMA_VERSION,
                oracle_schema_version: document.schema_version,
                game: document.game.clone(),
                skin: document.skin.clone(),
                metrics: document.metrics,
                paths: document
                    .paths
                    .into_iter()
                    .map(|mut path| {
                        path.path = path
                            .path
                            .as_deref()
                            .map(|value| portable_resource_path(&root, value))
                            .transpose()?;
                        Ok(path)
                    })
                    .collect::<Result<_, Error>>()?,
            };
            write_json(&output_dir.join(&relative_fixture), &fixture)?;
            fixtures.push(ManifestFixture {
                game: fixture.game,
                skin: fixture.skin,
                fixture: slash_path(&relative_fixture),
                metrics: fixture.metrics.len(),
                paths: fixture.paths.len(),
            });
        }
        games.push(ManifestGame {
            game: game.into(),
            buttons: buttons.iter().map(|button| (*button).into()).collect(),
            skins: inventory,
        });
    }

    let manifest = Manifest {
        fixture_schema_version: FIXTURE_SCHEMA_VERSION,
        oracle_schema_version: noteskin_oracle::SCHEMA_VERSION,
        harness_version: env!("CARGO_PKG_VERSION"),
        corpus: Corpus {
            metrics: "every section/key authored by a bundled metrics.ini",
            paths: "canonical loader elements plus every bundled button/element filename",
            source_snapshot:
                "exact paths and INI/redirect text; other resources are zero-byte placeholders",
        },
        itgmania: Itgmania {
            git_revision: env!("ITGMANIA_GIT_REVISION"),
            git_dirty: env!("ITGMANIA_GIT_DIRTY"),
        },
        games,
        fixtures,
        source_files,
    };
    write_json(&output_dir.join("_manifest.json"), &manifest)?;
    Ok(Report {
        skins: manifest.fixtures.len(),
        metrics: manifest.fixtures.len() * metrics.len(),
        paths: path_count,
        diagnostics: diagnostic_count,
    })
}

fn canonical_root(root: &Path) -> Result<PathBuf, Error> {
    let root = root
        .canonicalize()
        .map_err(|source| Error::io("open NoteSkins directory", root, source))?;
    let is_noteskins = root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("NoteSkins"));
    if !is_noteskins {
        return Err(Error::InvalidRoot(root));
    }
    Ok(root)
}

fn game_buttons(game: &str) -> Option<&'static [&'static str]> {
    GAMES
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(game))
        .map(|(_, buttons)| *buttons)
}

fn metric_queries(root: &Path) -> Result<Vec<MetricQuery>, Error> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    let mut queries = BTreeMap::new();
    for path in files.into_iter().filter(|path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("metrics.ini"))
    }) {
        let source = fs::read_to_string(&path)
            .map_err(|error| Error::io("read noteskin metrics", &path, error))?;
        let mut section = String::new();
        for raw_line in source.lines() {
            let line = raw_line.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') && line.len() > 2 {
                section = line[1..line.len() - 1].trim().to_string();
                continue;
            }
            let Some((key, _)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            queries
                .entry((section.to_ascii_lowercase(), key.to_ascii_lowercase()))
                .or_insert_with(|| MetricQuery {
                    section: section.clone(),
                    key: key.into(),
                });
        }
    }
    Ok(queries.into_values().collect())
}

fn path_queries(root: &Path, game: &str, buttons: &[&str]) -> Result<Vec<PathQuery>, Error> {
    let mut queries = BTreeMap::new();
    for &button in buttons {
        for &element in CORE_ELEMENTS {
            insert_path_query(&mut queries, button, element);
        }
    }

    for relative in [Path::new(game), Path::new("common")] {
        let directory = root.join(relative);
        if !directory.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        collect_files(&directory, &mut files)?;
        for path in files {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let stem = trim_variant_suffix(name);
            if stem.is_empty() {
                continue;
            }
            if let Some(element) = strip_prefix_ascii_case(stem, "Fallback ") {
                for &button in buttons {
                    insert_path_query(&mut queries, button, element.trim());
                }
                continue;
            }
            let mut matched = false;
            for &button in buttons {
                let prefix = format!("{button} ");
                if let Some(element) = strip_prefix_ascii_case(stem, &prefix) {
                    insert_path_query(&mut queries, button, element.trim());
                    matched = true;
                    break;
                }
            }
            if !matched {
                insert_path_query(&mut queries, "", stem);
            }
        }
    }
    Ok(queries.into_values().collect())
}

fn insert_path_query(
    queries: &mut BTreeMap<(String, String), PathQuery>,
    button: &str,
    element: &str,
) {
    if element.is_empty() {
        return;
    }
    queries
        .entry((button.to_ascii_lowercase(), element.to_ascii_lowercase()))
        .or_insert_with(|| PathQuery {
            button: button.into(),
            element: element.into(),
        });
}

fn trim_variant_suffix(name: &str) -> &str {
    let stem = name.rsplit_once('.').map_or(name, |(head, _)| head).trim();
    let no_paren = stem
        .rsplit_once(" (")
        .map_or(stem, |(head, _)| head)
        .trim_end();
    match no_paren.rsplit_once(' ') {
        Some((head, tail))
            if tail
                .split_once('x')
                .is_some_and(|(width, height)| digits_only(width) && digits_only(height)) =>
        {
            head.trim_end()
        }
        _ => no_paren,
    }
}

fn digits_only(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn strip_prefix_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    value
        .get(..prefix.len())
        .filter(|start| start.eq_ignore_ascii_case(prefix))
        .map(|_| &value[prefix.len()..])
}

fn skin_dirs(root: &Path, game: &str) -> Result<Vec<String>, Error> {
    let game_dir = root.join(game);
    let entries = fs::read_dir(&game_dir)
        .map_err(|source| Error::io("read noteskin game directory", &game_dir, source))?;
    let mut skins = Vec::new();
    for entry in entries {
        let entry =
            entry.map_err(|source| Error::io("read noteskin game directory", &game_dir, source))?;
        if entry
            .file_type()
            .map_err(|source| Error::io("inspect noteskin entry", &entry.path(), source))?
            .is_dir()
        {
            skins.push(entry.file_name().to_string_lossy().to_ascii_lowercase());
        }
    }
    skins.sort();
    Ok(skins)
}

fn source_snapshot(root: &Path) -> Result<Vec<SourceFile>, Error> {
    let mut files = Vec::new();
    collect_files(root, &mut files)?;
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let relative = relative_path(root, &path)?;
            let metadata = fs::metadata(&path)
                .map_err(|source| Error::io("inspect noteskin source", &path, source))?;
            let text = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("ini") || extension.eq_ignore_ascii_case("redir")
                })
                .then(|| {
                    fs::read_to_string(&path)
                        .map_err(|source| Error::io("read noteskin source", &path, source))
                })
                .transpose()?;
            Ok(SourceFile {
                path: relative,
                bytes: metadata.len(),
                text,
            })
        })
        .collect()
}

fn collect_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), Error> {
    let entries = fs::read_dir(directory)
        .map_err(|source| Error::io("read noteskin directory", directory, source))?;
    for entry in entries {
        let entry =
            entry.map_err(|source| Error::io("read noteskin directory", directory, source))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, output)?;
        } else if path.is_file() {
            output.push(path);
        }
    }
    Ok(())
}

fn portable_resource_path(root: &Path, value: &str) -> Result<String, Error> {
    let path = Path::new(value);
    if path.is_absolute() {
        return relative_path(root, path);
    }
    let portable = value.replace('\\', "/");
    if portable
        .get(.."NoteSkins/".len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("NoteSkins/"))
    {
        return Ok(portable["NoteSkins/".len()..].to_string());
    }
    Err(Error::InvalidResource(value.into()))
}

fn relative_path(root: &Path, path: &Path) -> Result<String, Error> {
    path.strip_prefix(root)
        .map(slash_path)
        .map_err(|_| Error::OutsideRoot {
            root: root.into(),
            path: path.into(),
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
    fs::write(path, bytes).map_err(|source| Error::io("write noteskin fixture", path, source))
}

#[derive(Debug)]
pub enum Error {
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    InvalidRoot(PathBuf),
    UnsupportedGame(String),
    InvalidResource(String),
    OutsideRoot {
        root: PathBuf,
        path: PathBuf,
    },
    Oracle {
        game: String,
        skin: String,
        source: noteskin_oracle::Error,
    },
    Serialize(serde_json::Error),
}

impl Error {
    fn io(action: &'static str, path: &Path, source: std::io::Error) -> Self {
        Self::Io {
            action,
            path: path.into(),
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
            Self::InvalidRoot(path) => write!(
                formatter,
                "noteskin root must be an existing directory named NoteSkins: {}",
                path.display()
            ),
            Self::UnsupportedGame(game) => write!(formatter, "unsupported noteskin game `{game}`"),
            Self::InvalidResource(path) => {
                write!(
                    formatter,
                    "ITGmania returned an unexpected noteskin path `{path}`"
                )
            }
            Self::OutsideRoot { root, path } => write!(
                formatter,
                "noteskin resource {} is outside {}",
                path.display(),
                root.display()
            ),
            Self::Oracle { game, skin, source } => {
                write!(formatter, "noteskin fixture {game}/{skin} failed: {source}")
            }
            Self::Serialize(error) => {
                write!(formatter, "could not serialize noteskin fixture: {error}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trims_texture_variant_hints() {
        assert_eq!(
            trim_variant_suffix("Down Tap Note (res 64x64).png"),
            "Down Tap Note"
        );
        assert_eq!(trim_variant_suffix("Tap Note 8x1.png"), "Tap Note");
    }

    #[test]
    fn slash_paths_are_portable() {
        assert_eq!(slash_path(Path::new("dance\\default")), "dance/default");
    }
}
