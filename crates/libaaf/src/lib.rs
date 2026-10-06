//! Native, read-only Advanced Authoring Format reader.
//!
//! Object IDs remain stable for the lifetime of a document. Unknown properties
//! retain their original bytes. Embedded streams borrow the compound-file
//! backing storage and can be copied without allocating a second media buffer.
#![forbid(unsafe_code)]

mod bytes;
pub mod cfb;
pub mod defs;
pub mod document;
pub mod inspection;
pub mod media;
pub mod timeline;
pub use document::{Document, Object, ObjectId, Property, ReaderOptions};
use std::{fmt, io};
pub use timeline::{Composition, Rational};

/// An AAF identifier in the on-disk GUID byte order.
#[derive(Clone, Copy, Default, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct Uid(pub [u8; 16]);
impl Uid {
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        Some(Self(bytes.get(..16)?.try_into().ok()?))
    }
    pub fn name(&self) -> Option<&'static str> {
        defs::UID_NAMES
            .iter()
            .find(|(id, _)| id == self)
            .map(|(_, name)| *name)
    }
}
impl fmt::Display for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let b = &self.0;
        write!(
            f,
            "{:08x}-{:04x}-{:04x}-{:02x}{:02x}-",
            u32::from_le_bytes(b[..4].try_into().unwrap()),
            u16::from_le_bytes(b[4..6].try_into().unwrap()),
            u16::from_le_bytes(b[6..8].try_into().unwrap()),
            b[8],
            b[9]
        )?;
        for v in &b[10..] {
            write!(f, "{v:02x}")?;
        }
        Ok(())
    }
}
impl fmt::Debug for Uid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(n) = self.name() {
            write!(f, "{n}")
        } else {
            fmt::Display::fmt(self, f)
        }
    }
}

/// Structured parser, media, or filesystem failure.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Invalid {
        context: &'static str,
        offset: usize,
    },
    Cycle {
        context: &'static str,
        id: u32,
    },
    Missing {
        context: &'static str,
        name: String,
    },
    Unsupported(String),
    Limit {
        context: &'static str,
        limit: usize,
    },
}
impl Error {
    pub(crate) fn invalid(context: &'static str, offset: usize) -> Self {
        Self::Invalid { context, offset }
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Invalid { context, offset } => write!(f, "invalid {context} at byte {offset}"),
            Self::Cycle { context, id } => write!(f, "cyclic {context} at {id}"),
            Self::Missing { context, name } => write!(f, "missing {context}: {name}"),
            Self::Unsupported(s) => write!(f, "unsupported {s}"),
            Self::Limit { context, limit } => write!(f, "{context} exceeds limit {limit}"),
        }
    }
}
impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Io(e) = self {
            Some(e)
        } else {
            None
        }
    }
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Diagnostic {
    pub object: Option<ObjectId>,
    pub message: String,
}
