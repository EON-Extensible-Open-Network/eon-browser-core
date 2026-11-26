// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 EON contributors

use std::fmt;

/// Everything the core can fail with. Each variant says what a person (or the application)
/// can do about it; none of them is a panic.
#[derive(Debug)]
pub enum Error {
    /// Reading or writing the profile folder.
    Io(std::io::Error),
    /// The profile database.
    Db(rusqlite::Error),
    /// A stored JSON document (list state, session) could not be read.
    Json(serde_json::Error),
    /// A list the application fetched is not a filter list (an error page, a captive
    /// portal, a truncated download). It was not stored; the previous copy is kept.
    NotAList { list: String, reason: String },
    /// A list id that is not in the catalogue and is not the person's own.
    UnknownList(String),
    /// The application's fetch function failed for this list.
    Fetch { list: String, reason: String },
    /// An address that cannot be used where one was required.
    BadUrl(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "file error: {e}"),
            Error::Db(e) => write!(f, "profile database error: {e}"),
            Error::Json(e) => write!(f, "stored data could not be read: {e}"),
            Error::NotAList { list, reason } => {
                write!(
                    f,
                    "what was downloaded for {list} is not a filter list: {reason}"
                )
            }
            Error::UnknownList(id) => write!(f, "no filter list called {id}"),
            Error::Fetch { list, reason } => write!(f, "could not download {list}: {reason}"),
            Error::BadUrl(u) => write!(f, "not a usable address: {u}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Error::Db(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}
