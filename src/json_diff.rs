use serde_json::{Number, Value};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq, Eq)]
pub struct Difference {
    pub path: String,
    pub expected: String,
    pub actual: String,
}

impl fmt::Display for Difference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: expected {}, actual {}",
            self.path, self.expected, self.actual
        )
    }
}

pub fn read(path: &Path) -> Result<Value, Error> {
    let bytes = fs::read(path).map_err(|source| Error::Io {
        path: path.to_owned(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| Error::Parse {
        path: path.to_owned(),
        source,
    })
}

pub fn compare(expected: &Value, actual: &Value) -> Vec<Difference> {
    let mut differences = Vec::new();
    compare_at("$", Some(expected), Some(actual), &mut differences);
    differences
}

fn compare_at(
    path: &str,
    expected: Option<&Value>,
    actual: Option<&Value>,
    differences: &mut Vec<Difference>,
) {
    match (expected, actual) {
        (Some(Value::Object(expected)), Some(Value::Object(actual))) => {
            let keys = expected
                .keys()
                .chain(actual.keys())
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            for key in keys {
                compare_at(
                    &key_path(path, key),
                    expected.get(key),
                    actual.get(key),
                    differences,
                );
            }
        }
        (Some(Value::Array(expected)), Some(Value::Array(actual))) => {
            for index in 0..expected.len().max(actual.len()) {
                compare_at(
                    &format!("{path}[{index}]"),
                    expected.get(index),
                    actual.get(index),
                    differences,
                );
            }
        }
        (Some(Value::Number(expected)), Some(Value::Number(actual)))
            if numbers_equal(expected, actual) => {}
        (Some(expected), Some(actual)) if expected == actual => {}
        (expected, actual) => differences.push(Difference {
            path: path.to_owned(),
            expected: render(expected),
            actual: render(actual),
        }),
    }
}

fn numbers_equal(expected: &Number, actual: &Number) -> bool {
    match (
        expected.as_i64(),
        expected.as_u64(),
        expected.as_f64(),
        actual.as_i64(),
        actual.as_u64(),
        actual.as_f64(),
    ) {
        (Some(left), _, _, Some(right), _, _) => left == right,
        (_, Some(left), _, _, Some(right), _) => left == right,
        (Some(integer), _, _, _, _, Some(float)) => signed_equals_float(integer, float),
        (_, Some(integer), _, _, _, Some(float)) => unsigned_equals_float(integer, float),
        (_, _, Some(float), Some(integer), _, _) => signed_equals_float(integer, float),
        (_, _, Some(float), _, Some(integer), _) => unsigned_equals_float(integer, float),
        (_, _, Some(left), _, _, Some(right)) => left.to_bits() == right.to_bits(),
        _ => false,
    }
}

fn signed_equals_float(integer: i64, float: f64) -> bool {
    const TWO_TO_63: f64 = 9_223_372_036_854_775_808.0;
    !(float == 0.0 && float.is_sign_negative())
        && float.fract() == 0.0
        && (-TWO_TO_63..TWO_TO_63).contains(&float)
        && float as i64 == integer
}

fn unsigned_equals_float(integer: u64, float: f64) -> bool {
    const TWO_TO_64: f64 = 18_446_744_073_709_551_616.0;
    !(float == 0.0 && float.is_sign_negative())
        && float.fract() == 0.0
        && (0.0..TWO_TO_64).contains(&float)
        && float as u64 == integer
}

fn key_path(parent: &str, key: &str) -> String {
    if key.bytes().enumerate().all(|(index, byte)| {
        byte == b'_' || byte.is_ascii_alphabetic() || index > 0 && byte.is_ascii_digit()
    }) && !key.is_empty()
    {
        format!("{parent}.{key}")
    } else {
        format!("{parent}[{}]", Value::String(key.to_owned()))
    }
}

fn render(value: Option<&Value>) -> String {
    value.map_or_else(|| "<missing>".into(), Value::to_string)
}

#[derive(Debug)]
pub enum Error {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(
                    formatter,
                    "could not read JSON {}: {source}",
                    path.display()
                )
            }
            Self::Parse { path, source } => {
                write!(formatter, "invalid JSON in {}: {source}", path.display())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ignores_equivalent_number_spellings() {
        let expected: Value = serde_json::from_str(r#"{"a":1,"b":125.0,"c":1e3}"#).unwrap();
        let actual: Value = serde_json::from_str(r#"{"a":1.0,"b":1.25e2,"c":1000}"#).unwrap();
        assert!(compare(&expected, &actual).is_empty());
    }

    #[test]
    fn retains_negative_zero_fidelity() {
        let expected: Value = serde_json::from_str(r#"{"value":-0.0}"#).unwrap();
        let actual: Value = serde_json::from_str(r#"{"value":0}"#).unwrap();
        assert_eq!(
            compare(&expected, &actual),
            [Difference {
                path: "$.value".into(),
                expected: "-0.0".into(),
                actual: "0".into(),
            }]
        );
    }

    #[test]
    fn reports_stable_nested_paths() {
        let differences = compare(
            &json!({"chart": [{"meter": 9}], "expected only": true}),
            &json!({"chart": [{"meter": 10}], "actual": null}),
        );
        assert_eq!(
            differences,
            [
                Difference {
                    path: "$.actual".into(),
                    expected: "<missing>".into(),
                    actual: "null".into(),
                },
                Difference {
                    path: "$.chart[0].meter".into(),
                    expected: "9".into(),
                    actual: "10".into(),
                },
                Difference {
                    path: "$[\"expected only\"]".into(),
                    expected: "true".into(),
                    actual: "<missing>".into(),
                },
            ]
        );
    }

    #[test]
    fn does_not_round_large_integers_into_floats() {
        let expected: Value = serde_json::from_str("9223372036854775807").unwrap();
        let actual: Value = serde_json::from_str("9223372036854775808.0").unwrap();
        assert_eq!(compare(&expected, &actual).len(), 1);
    }
}
