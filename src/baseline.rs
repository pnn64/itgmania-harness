use crate::json_diff::{self, Difference};
use crate::oracle;
use crate::selector::{self, Selector};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MANIFEST_VERSION: u32 = 2;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputManifest {
    version: u32,
    case: Vec<InputCase>,
    theme: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct InputCase {
    name: String,
    simfile: String,
    index: Option<usize>,
    steps_type: Option<String>,
    difficulty_code: Option<i32>,
    description: Option<String>,
    #[serde(default)]
    all_charts: bool,
    expected_chart_count: Option<usize>,
    oracle_simfile: Option<String>,
    repair_reason: Option<String>,
    original_sha256: Option<String>,
    oracle_sha256: Option<String>,
    #[serde(default)]
    expected_diagnostics: Vec<String>,
}

#[derive(Debug, Serialize)]
struct Provenance<'a> {
    baseline_manifest_version: u32,
    oracle_schema_version: u32,
    harness_version: &'a str,
    input_manifest: String,
    itgmania: ItgmaniaProvenance<'a>,
    cases: Vec<OutputCase<'a>>,
}

#[derive(Debug, Serialize)]
struct ItgmaniaProvenance<'a> {
    build_root: &'a str,
    git_revision: &'a str,
    git_dirty: &'a str,
}

#[derive(Debug, Serialize)]
struct OutputCase<'a> {
    name: &'a str,
    simfile: &'a str,
    output: String,
    original_sha256: &'a str,
    oracle_sha256: &'a str,
    oracle_simfile: &'a str,
    repair_reason: Option<&'a str>,
    source_indices: Vec<u32>,
}

pub fn generate(manifest_path: &Path, output_dir: &Path) -> Result<usize, Error> {
    let generated = evaluate(manifest_path)?;

    fs::create_dir_all(output_dir)
        .map_err(|source| Error::io("create output directory", output_dir, source))?;
    for case in &generated {
        atomic_json(
            &output_dir.join(format!("{}.json", case.input.name)),
            &case.document,
        )?;
    }

    let provenance = Provenance {
        baseline_manifest_version: MANIFEST_VERSION,
        oracle_schema_version: oracle::SCHEMA_VERSION,
        harness_version: env!("CARGO_PKG_VERSION"),
        input_manifest: manifest_path.to_string_lossy().into_owned(),
        itgmania: ItgmaniaProvenance {
            build_root: env!("ITGMANIA_BUILD_ROOT"),
            git_revision: env!("ITGMANIA_GIT_REVISION"),
            git_dirty: env!("ITGMANIA_GIT_DIRTY"),
        },
        cases: generated
            .iter()
            .map(|case| OutputCase {
                name: &case.input.name,
                simfile: &case.input.simfile,
                output: format!("{}.json", case.input.name),
                original_sha256: &case.original_sha256,
                oracle_sha256: &case.document.source_sha256,
                oracle_simfile: case
                    .input
                    .oracle_simfile
                    .as_deref()
                    .unwrap_or(&case.input.simfile),
                repair_reason: case.input.repair_reason.as_deref(),
                source_indices: case
                    .document
                    .charts
                    .iter()
                    .map(|chart| chart.source_index)
                    .collect(),
            })
            .collect(),
    };
    atomic_json(&output_dir.join("_manifest.json"), &provenance)?;
    Ok(generated.len())
}

pub fn verify(manifest_path: &Path, expected_dir: &Path) -> Result<usize, Error> {
    let generated = evaluate(manifest_path)?;
    let mut failures = Vec::new();
    for case in &generated {
        let path = expected_dir.join(format!("{}.json", case.input.name));
        let expected = json_diff::read(&path).map_err(Error::Json)?;
        let actual_json = serde_json::to_vec(&case.document).map_err(Error::Serialize)?;
        let actual = serde_json::from_slice(&actual_json).map_err(Error::Serialize)?;
        let differences = json_diff::compare(&expected, &actual);
        if !differences.is_empty() {
            failures.push(VerificationFailure {
                case: case.input.name.clone(),
                differences,
            });
        }
    }
    if failures.is_empty() {
        Ok(generated.len())
    } else {
        Err(Error::Verification(failures))
    }
}

struct EvaluatedCase {
    input: InputCase,
    document: oracle::Document,
    original_sha256: String,
}

fn evaluate(manifest_path: &Path) -> Result<Vec<EvaluatedCase>, Error> {
    let manifest_text = fs::read_to_string(manifest_path)
        .map_err(|source| Error::io("read manifest", manifest_path, source))?;
    let manifest: InputManifest = toml::from_str(&manifest_text).map_err(Error::ParseManifest)?;
    if !matches!(manifest.version, 1 | MANIFEST_VERSION) {
        return Err(Error::ManifestVersion(manifest.version));
    }
    validate_names(&manifest.case)?;
    if manifest.case.is_empty() {
        return Err(Error::Capture("manifest contains no cases".into()));
    }

    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut generated = Vec::with_capacity(manifest.case.len());
    let theme = manifest.theme.as_ref().map(|path| manifest_dir.join(path));
    for case in manifest.case {
        if manifest.version == 2 && case.all_charts && case.expected_chart_count.is_none() {
            return Err(Error::Capture(format!(
                "baseline `{}` whole-simfile capture requires expected_chart_count",
                case.name
            )));
        }
        let source_path = manifest_dir.join(&case.simfile);
        let original_bytes =
            oracle::source_bytes(&source_path).map_err(|source| Error::Oracle {
                case: case.name.clone(),
                source,
            })?;
        let original_sha256 = oracle::sha256(&original_bytes);
        let repaired = case.oracle_simfile.is_some();
        if repaired
            && (case
                .repair_reason
                .as_ref()
                .is_none_or(|reason| reason.trim().is_empty())
                || case.original_sha256.is_none()
                || case.oracle_sha256.is_none())
        {
            return Err(Error::Capture(format!(
                "baseline `{}` oracle repair requires repair_reason, original_sha256 and oracle_sha256",
                case.name
            )));
        }
        if !repaired && (case.repair_reason.is_some() || case.oracle_sha256.is_some()) {
            return Err(Error::Capture(format!(
                "baseline `{}` repair metadata requires oracle_simfile",
                case.name
            )));
        }
        if case
            .original_sha256
            .as_ref()
            .is_some_and(|expected| expected != &original_sha256)
        {
            return Err(Error::Capture(format!(
                "baseline `{}` original SHA-256 does not match",
                case.name
            )));
        }
        let oracle_path =
            manifest_dir.join(case.oracle_simfile.as_deref().unwrap_or(&case.simfile));
        let mut document =
            oracle::load_with_theme(&oracle_path, theme.as_deref()).map_err(|source| {
                Error::Oracle {
                    case: case.name.clone(),
                    source,
                }
            })?;
        let expected_hash = case.oracle_sha256.as_ref().unwrap_or(&original_sha256);
        if &document.source_sha256 != expected_hash {
            return Err(Error::Capture(format!(
                "baseline `{}` oracle SHA-256 does not match",
                case.name
            )));
        }
        if case
            .expected_chart_count
            .is_some_and(|count| count != document.charts.len())
        {
            return Err(Error::Capture(format!(
                "baseline `{}` expected {} charts, ITGmania loaded {}",
                case.name,
                case.expected_chart_count.unwrap(),
                document.charts.len()
            )));
        }
        if document.diagnostics != case.expected_diagnostics {
            return Err(Error::Capture(format!(
                "baseline `{}` unexpected loader diagnostics: {:?}",
                case.name, document.diagnostics
            )));
        }
        let selector = Selector {
            index: case.index,
            steps_type: case.steps_type.clone(),
            difficulty_code: case.difficulty_code,
            description: case.description.clone(),
        };
        if case.all_charts {
            if case.index.is_some()
                || case.steps_type.is_some()
                || case.difficulty_code.is_some()
                || case.description.is_some()
            {
                return Err(Error::Capture(format!(
                    "baseline `{}` all_charts cannot be combined with selectors",
                    case.name
                )));
            }
        } else {
            let index = selector
                .select(&document.charts)
                .map_err(|source| Error::Selector {
                    case: case.name.clone(),
                    source,
                })?;
            document.charts = vec![document.charts.swap_remove(index)];
        }
        for chart in &document.charts {
            validate_capture(&case.name, chart)?;
        }
        if repaired {
            document.repair = Some(oracle::Repair {
                original_sha256: original_sha256.clone(),
                oracle_simfile: case.oracle_simfile.as_ref().unwrap().replace('\\', "/"),
                oracle_sha256: document.source_sha256.clone(),
                reason: case.repair_reason.as_ref().unwrap().clone(),
            });
        }
        document.simfile = case.simfile.replace('\\', "/");
        if let Some(theme) = &mut document.theme {
            theme.path = manifest.theme.as_ref().unwrap().replace('\\', "/");
        }
        generated.push(EvaluatedCase {
            input: case,
            document,
            original_sha256,
        });
    }
    Ok(generated)
}

fn validate_capture(case: &str, chart: &oracle::Chart) -> Result<(), Error> {
    let hash = &chart.groove_stats_hash;
    let stats_valid = chart.player_stats.iter().all(|stats| {
        stats.notes_per_measure.len() == stats.nps_per_measure.len()
            && stats
                .nps_per_measure
                .iter()
                .all(|nps| nps.get().is_finite() && nps.get() >= 0.0)
            && stats.peak_nps.get().is_finite()
            && stats
                .radar
                .iter()
                .chain(&stats.tech_counts)
                .all(|value| value.get().is_finite())
    });
    if !chart.note_data_supported
        || hash.len() != 16
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        || !stats_valid
    {
        return Err(Error::Capture(format!(
            "baseline `{case}` chart {} has incomplete native hash/statistics",
            chart.source_index
        )));
    }
    if let Some(theme) = &chart.theme {
        if theme["status"] != "complete" {
            return Err(Error::Capture(format!(
                "baseline `{case}` chart {} theme capture is {}: {}",
                chart.source_index, theme["status"], theme["errors"]
            )));
        }
    }
    Ok(())
}

fn validate_names(cases: &[InputCase]) -> Result<(), Error> {
    let mut names = HashSet::with_capacity(cases.len());
    for case in cases {
        if case.name.is_empty()
            || !case
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        {
            return Err(Error::InvalidName(case.name.clone()));
        }
        if !names.insert(&case.name) {
            return Err(Error::DuplicateName(case.name.clone()));
        }
    }
    Ok(())
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), Error> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(Error::Serialize)?;
    bytes.push(b'\n');
    let temp = temp_path(path);
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|source| Error::io("create temporary output", &temp, source))?;
        file.write_all(&bytes)
            .map_err(|source| Error::io("write temporary output", &temp, source))?;
        file.sync_all()
            .map_err(|source| Error::io("flush temporary output", &temp, source))?;
        fs::rename(&temp, path).map_err(|source| Error::io("replace output", path, source))
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

fn temp_path(path: &Path) -> PathBuf {
    let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("baseline");
    path.with_file_name(format!(".{name}.tmp-{}-{id}", std::process::id()))
}

#[derive(Debug)]
pub enum Error {
    Io {
        action: &'static str,
        path: PathBuf,
        source: std::io::Error,
    },
    ParseManifest(toml::de::Error),
    ManifestVersion(u32),
    InvalidName(String),
    DuplicateName(String),
    Capture(String),
    Oracle {
        case: String,
        source: oracle::Error,
    },
    Selector {
        case: String,
        source: selector::Error,
    },
    Json(json_diff::Error),
    Serialize(serde_json::Error),
    Verification(Vec<VerificationFailure>),
}

#[derive(Debug)]
pub struct VerificationFailure {
    case: String,
    differences: Vec<Difference>,
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
            Self::ParseManifest(error) => write!(formatter, "invalid baseline manifest: {error}"),
            Self::ManifestVersion(version) => write!(
                formatter,
                "unsupported baseline manifest version {version}; expected 1 or {MANIFEST_VERSION}"
            ),
            Self::InvalidName(name) => write!(
                formatter,
                "invalid baseline name `{name}`; use ASCII letters, digits, `-`, or `_`"
            ),
            Self::DuplicateName(name) => write!(formatter, "duplicate baseline name `{name}`"),
            Self::Capture(message) => formatter.write_str(message),
            Self::Oracle { case, source } => {
                write!(formatter, "baseline `{case}` failed: {source}")
            }
            Self::Selector { case, source } => {
                write!(formatter, "baseline `{case}` selection failed: {source}")
            }
            Self::Json(error) => error.fmt(formatter),
            Self::Serialize(error) => write!(formatter, "could not serialize baseline: {error}"),
            Self::Verification(failures) => {
                write!(formatter, "baseline verification failed:")?;
                for failure in failures {
                    write!(formatter, "\n  baseline `{}`:", failure.case)?;
                    for difference in &failure.differences {
                        write!(formatter, "\n    {difference}")?;
                    }
                }
                Ok(())
            }
        }
    }
}
