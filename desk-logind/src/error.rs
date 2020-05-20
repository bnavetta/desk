//! Logind error type
use std::backtrace::Backtrace;

use dbus::Error as DBusError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LogindError {
    #[error("XDG_SESSION_ID not set")]
    NoSessionId { backtrace: Backtrace },

    #[error("Could not subscribe to {signal}")]
    MatchFailed {
        signal: &'static str,
        #[source]
        source: DBusError,
        backtrace: Backtrace,
    },

    // TODO: add more specific error cases as they come up
    #[error("D-Bus operation failed")]
    DBusError {
        #[from]
        #[source]
        source: DBusError,
        backtrace: Backtrace,
    },

    #[error("{message}")]
    InhibitorFileError {
        message: String,
        #[source]
        source: nix::Error,
        backtrace: Backtrace
    }
}

impl LogindError {
    pub fn no_session_id() -> LogindError {
        LogindError::NoSessionId {
            backtrace: Backtrace::capture(),
        }
    }

    pub fn match_failed(signal: &'static str, error: DBusError) -> LogindError {
        LogindError::MatchFailed {
            signal,
            source: error,
            backtrace: Backtrace::capture(),
        }
    }

    pub fn inhibitor_file_error(message: String, error: nix::Error) -> LogindError {
        LogindError::InhibitorFileError {
            message,
            source: error,
            backtrace: Backtrace::capture()
        }
    }
}
