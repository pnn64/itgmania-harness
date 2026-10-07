use crate::oracle::Chart;
use std::fmt;

#[derive(Debug, Default)]
pub struct Selector {
    pub index: Option<usize>,
    pub steps_type: Option<String>,
    pub difficulty_code: Option<i32>,
    pub description: Option<String>,
}

impl Selector {
    pub fn validate(&self) -> Result<(), Error> {
        if self.index.is_some() && self.has_semantic_fields() {
            return Err(Error::Conflict);
        }
        Ok(())
    }

    pub fn select(&self, charts: &[Chart]) -> Result<usize, Error> {
        self.validate()?;
        if let Some(index) = self.index {
            return (index < charts.len())
                .then_some(index)
                .ok_or(Error::IndexOutOfRange {
                    index,
                    chart_count: charts.len(),
                });
        }
        if !self.has_semantic_fields() {
            return match charts.len() {
                0 => Err(Error::NoMatch),
                1 => Ok(0),
                _ => Err(Error::MultipleMatches((0..charts.len()).collect())),
            };
        }

        let matches = charts
            .iter()
            .enumerate()
            .filter(|(_, chart)| {
                self.steps_type
                    .as_ref()
                    .is_none_or(|value| chart.steps_type.source == *value)
                    && self
                        .difficulty_code
                        .is_none_or(|value| chart.difficulty_code == value)
                    && self
                        .description
                        .as_ref()
                        .is_none_or(|value| chart.description == *value)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        match matches.as_slice() {
            [index] => Ok(*index),
            [] => Err(Error::NoMatch),
            _ => Err(Error::MultipleMatches(matches)),
        }
    }

    fn has_semantic_fields(&self) -> bool {
        self.steps_type.is_some() || self.difficulty_code.is_some() || self.description.is_some()
    }
}

#[derive(Debug)]
pub enum Error {
    Conflict,
    IndexOutOfRange { index: usize, chart_count: usize },
    NoMatch,
    MultipleMatches(Vec<usize>),
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Conflict => write!(
                formatter,
                "--index cannot be combined with semantic chart selectors"
            ),
            Self::IndexOutOfRange { index, chart_count } => write!(
                formatter,
                "chart index {index} is out of range for {chart_count} charts"
            ),
            Self::NoMatch => write!(formatter, "chart selectors matched no charts"),
            Self::MultipleMatches(indices) => {
                let indices = indices
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    formatter,
                    "chart selectors matched multiple charts at indices {indices}"
                )
            }
        }
    }
}
