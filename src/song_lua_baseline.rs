use crate::song_lua_oracle;
use serde::Serialize;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

const FIXTURE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub struct Report {
    pub simfiles: usize,
    pub lua_entries: usize,
}

#[derive(Serialize)]
struct Manifest<'a> {
    fixture_schema_version: u32,
    oracle_schema_version: u32,
    harness_version: &'a str,
    itgmania: Itgmania<'a>,
    simfiles: Vec<ManifestSimfile>,
}

#[derive(Serialize)]
struct Itgmania<'a> {
    git_revision: &'a str,
    git_dirty: &'a str,
}

#[derive(Serialize)]
struct ManifestSimfile {
    simfile: String,
    fixture: String,
    title: String,
    lua_entries: usize,
}

pub fn generate(songs_root: &Path, output_dir: &Path) -> Result<Report, Error> {
    let songs_root = songs_root
        .canonicalize()
        .map_err(|source| Error::io("open song corpus", songs_root, source))?;
    let mut simfiles = Vec::new();
    collect_simfiles(&songs_root, &mut simfiles)?;
    simfiles.sort();
    if simfiles.is_empty() {
        return Err(Error::NoSimfiles(songs_root));
    }

    let mut entries = Vec::with_capacity(simfiles.len());
    let mut lua_entries = 0;
    for simfile in simfiles {
        let mut document = song_lua_oracle::load(&simfile).map_err(|source| Error::Oracle {
            path: simfile.clone(),
            source,
        })?;
        let relative_simfile = relative_path(&songs_root, &simfile)?;
        document.simfile = relative_simfile.clone();
        document.song_dir = simfile
            .parent()
            .map(|path| relative_path(&songs_root, path))
            .transpose()?
            .unwrap_or_default();
        for change in &mut document.changes {
            change.lua_entry = change
                .lua_entry
                .as_deref()
                .map(Path::new)
                .map(|path| relative_path(&songs_root, path))
                .transpose()?;
        }
        let count = document
            .changes
            .iter()
            .filter(|change| change.lua_entry.is_some())
            .count();
        lua_entries += count;
        let relative_fixture = Path::new(&relative_simfile).with_extension(format!(
            "{}.json",
            simfile
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("simfile")
        ));
        write_json(&output_dir.join(&relative_fixture), &document)?;
        entries.push(ManifestSimfile {
            simfile: relative_simfile,
            fixture: slash_path(&relative_fixture),
            title: document.title,
            lua_entries: count,
        });
    }

    let manifest = Manifest {
        fixture_schema_version: FIXTURE_SCHEMA_VERSION,
        oracle_schema_version: song_lua_oracle::SCHEMA_VERSION,
        harness_version: env!("CARGO_PKG_VERSION"),
        itgmania: Itgmania {
            git_revision: env!("ITGMANIA_GIT_REVISION"),
            git_dirty: env!("ITGMANIA_GIT_DIRTY"),
        },
        simfiles: entries,
    };
    write_json(&output_dir.join("_manifest.json"), &manifest)?;
    Ok(Report {
        simfiles: manifest.simfiles.len(),
        lua_entries,
    })
}

pub(crate) fn collect_simfiles(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), Error> {
    let entries = fs::read_dir(directory)
        .map_err(|source| Error::io("read song corpus", directory, source))?;
    for entry in entries {
        let entry = entry.map_err(|source| Error::io("read song corpus", directory, source))?;
        let path = entry.path();
        if path.is_dir() {
            collect_simfiles(&path, output)?;
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension.to_ascii_lowercase().as_str(),
                    "sm" | "sma" | "ssc" | "ats"
                )
            })
        {
            output.push(path);
        }
    }
    Ok(())
}

pub(crate) fn relative_path(root: &Path, path: &Path) -> Result<String, Error> {
    let path = if path.exists() {
        path.canonicalize()
            .map_err(|source| Error::io("open corpus path", path, source))?
    } else {
        path.to_owned()
    };
    path.strip_prefix(root)
        .map(slash_path)
        .map_err(|_| Error::OutsideRoot {
            root: root.to_owned(),
            path,
        })
}

pub(crate) fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn write_json(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|source| Error::io("create fixture directory", parent, source))?;
    let mut bytes = serde_json::to_vec_pretty(value).map_err(Error::Serialize)?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|source| Error::io("write song Lua fixture", path, source))
}

pub(crate) fn write_compact_json(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|source| Error::io("create fixture directory", parent, source))?;
    let mut bytes = serde_json::to_vec(value).map_err(Error::Serialize)?;
    bytes.push(b'\n');
    fs::write(path, bytes).map_err(|source| Error::io("write song Lua fixture", path, source))
}

#[derive(Debug)]
pub enum Error {
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    NoSimfiles(PathBuf),
    Oracle {
        path: PathBuf,
        source: song_lua_oracle::Error,
    },
    OutsideRoot {
        root: PathBuf,
        path: PathBuf,
    },
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
            Self::NoSimfiles(path) => write!(
                formatter,
                "no .sm, .sma, .ssc, or .ats files found under {}",
                path.display()
            ),
            Self::Oracle { path, source } => {
                write!(
                    formatter,
                    "song Lua fixture {} failed: {source}",
                    path.display()
                )
            }
            Self::OutsideRoot { root, path } => write!(
                formatter,
                "song Lua path {} is outside corpus root {}",
                path.display(),
                root.display()
            ),
            Self::Serialize(error) => {
                write!(formatter, "could not serialize song Lua fixture: {error}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simfile_extensions_are_preserved_in_fixture_names() {
        assert_eq!(
            Path::new("Song/file.ssc").with_extension("ssc.json"),
            PathBuf::from("Song/file.ssc.json")
        );
    }
}
