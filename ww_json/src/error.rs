//! Errors of the JSON codec: what failed and where, as a JSON path (`args.timeout`, `items[2].name`).

use std::fmt;

/// One step of a [JsonPath].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// An object member, `.name`.
    Field(String),
    /// A list or tuple element, `[2]`.
    Index(usize),
}

/// Where in the JSON document a value is: a root name (`args`, `value`, `items`) and the steps below it.
///
/// ```
/// use ww_json::{JsonPath, Step};
/// let mut path = JsonPath::new("items");
/// path.push(Step::Index(2));
/// path.push(Step::Field("name".into()));
/// assert_eq!(path.to_string(), "items[2].name");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonPath {
    pub root: String,
    pub steps: Vec<Step>,
}

impl JsonPath {
    pub fn new(root: impl Into<String>) -> Self {
        JsonPath {
            root: root.into(),
            steps: vec![],
        }
    }

    pub fn push(&mut self, step: Step) {
        self.steps.push(step);
    }

    pub fn pop(&mut self) {
        self.steps.pop();
    }
}

impl fmt::Display for JsonPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.root)?;
        for step in &self.steps {
            match step {
                Step::Field(name) => write!(f, ".{name}")?,
                Step::Index(idx) => write!(f, "[{idx}]")?,
            }
        }
        Ok(())
    }
}

/// What went wrong at a [JsonPath].
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum ErrorKind {
    /// The JSON value is of another kind than the type takes (`expected a string, found a number`).
    #[error("expected {expected}, found {found}")]
    WrongType {
        /// What the type takes, in words: `a boolean`, `an integer of u8`, `a list`.
        expected: String,
        /// The JSON kind found: `null`, `a boolean`, `a number`, `a string`, `a list`, `an object`.
        found: &'static str,
    },
    /// An integer or float outside the range of its type, or a number that is no number.
    #[error("{value} is out of range for {ty}")]
    OutOfRange { value: String, ty: String },
    /// A name that is no field of the struct, variant or argument list.
    #[error("no field `{name}`, expected one of: {}", .expected.join(", "))]
    UnknownField { name: String, expected: Vec<String> },
    /// A field or argument that is required but not given.
    #[error("field `{name}` is missing")]
    MissingField { name: String },
    /// A variant the enum doesn't have.
    #[error("{enum_name} has no variant `{name}`, expected one of: {}", .expected.join(", "))]
    UnknownVariant {
        enum_name: String,
        name: String,
        expected: Vec<String>,
    },
    /// A list, tuple, array or string of the wrong length.
    #[error("expected {expected}, got {got}")]
    Length { expected: String, got: usize },
    /// A quantity text or number that doesn't convert exactly into the type: wrong dimension, a fraction of the
    /// stored step, out of bounds. The text names the quantity and the type.
    #[error("{0}")]
    Quantity(String),
    /// A type or value the JSON form doesn't cover (`Opaque` values, `Flag`, unresolved generics).
    #[error("{0}")]
    Unsupported(String),
    /// The API model doesn't resolve the type (a missing definition).
    #[error("{0}")]
    Model(String),
    /// A value of another shape than its type, found while writing JSON: a bug on the producer's side.
    #[error("the value does not match its type: {0}")]
    Mismatch(String),
    /// Nested deeper than the dynamic decoder reads.
    #[error("nested deeper than {0} levels")]
    TooDeep(usize),
}

/// An error of the codec: the [ErrorKind] and the [JsonPath] of the failing value.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
#[error("{path}: {kind}")]
pub struct JsonError {
    pub path: JsonPath,
    pub kind: ErrorKind,
}

impl JsonError {
    pub(crate) fn new(path: &JsonPath, kind: ErrorKind) -> Self {
        JsonError {
            path: path.clone(),
            kind,
        }
    }
}
