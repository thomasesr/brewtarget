
impl ColorFormula {
    /// Every formula, in the order the C++ declares them.
    pub const ALL: [Self; 3] = [Self::Mosher, Self::Daniel, Self::Morey];
}

/// A string that names no colour formula.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a colour formula")]
pub struct NoSuchColorFormula(String);

impl std::str::FromStr for ColorFormula {
    type Err = NoSuchColorFormula;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|formula| formula.as_str() == value)
            .ok_or_else(|| NoSuchColorFormula(value.to_owned()))
    }
}
