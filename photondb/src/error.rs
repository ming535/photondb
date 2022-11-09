use thiserror::Error;

use crate::page_store::Error as PageError;

/// A list of possible errors returned by PhotonDB.
#[derive(Error, Debug)]
pub enum Error {
    /// Some data is corrupted.
    #[error("Corrupted")]
    Corrupted,
}

impl From<PageError> for Error {
    fn from(err: PageError) -> Self {
        match err {
            PageError::Corrupted => Self::Corrupted,
            e => unreachable!("unexpected error: {:?}", e),
        }
    }
}

/// A specialized [`Result`] type returned by PhotonDB.
pub type Result<T, E = Error> = std::result::Result<T, E>;