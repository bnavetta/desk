//! Model for systemd-logind inhibitor locks

use std::fmt;
use std::os::unix::io::{IntoRawFd, AsRawFd, RawFd};

use dbus::arg::OwnedFd;
use nix::unistd;

use crate::error::LogindError;

/// A logind event which can be inhibited (by taking an inhibitor lock)
#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub enum InhibitEvent {
    Shutdown,
    Sleep,
    Idle,
    HandlePowerKey,
    HandleSuspendKey,
    HandleHibernateKey,
    HandleLidSwitch,
}

impl InhibitEvent {
    pub fn as_str(self) -> &'static str {
        match self {
            InhibitEvent::Shutdown => "shutdown",
            InhibitEvent::Sleep => "sleep",
            InhibitEvent::Idle => "idle",
            InhibitEvent::HandlePowerKey => "handle-power-key",
            InhibitEvent::HandleSuspendKey => "handle-suspend-key",
            InhibitEvent::HandleHibernateKey => "handle-hibernate-key",
            InhibitEvent::HandleLidSwitch => "handle-lid-switch",
        }
    }
}

impl fmt::Display for InhibitEvent {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A set of events to inhibit.
#[derive(Default, Eq, PartialEq)]
pub struct InhibitEventSet(String);

impl InhibitEventSet {
    /// Creates a new, empty event set
    pub fn new() -> InhibitEventSet {
        InhibitEventSet(String::new())
    }

    /// Creates a new event set containing one event.
    pub fn with_event(event: InhibitEvent) -> InhibitEventSet {
        InhibitEventSet(format!("{}:", event.as_str()))
    }

    /// Add an event to the set. Note that this does not check if `event` is already included.
    pub fn add(&mut self, event: InhibitEvent) -> &mut InhibitEventSet {
        self.0.push_str(event.as_str());
        self.0.push(':');
        self
    }

    pub fn as_str(&self) -> &str {
        if self.0.is_empty() {
            ""
        } else {
            &self.0[0..self.0.len()]
        }
    }
}

#[derive(Debug, Eq, PartialEq, Copy, Clone, Hash)]
pub enum InhibitMode {
    /// An inhibitor lock which prevents the event from occurring.
    Block,
    /// An inhibitor lock which delays the inhibited event for a short period of time.
    Delay,
}

impl InhibitMode {
    pub fn as_str(self) -> &'static str {
        match self {
            InhibitMode::Block => "block",
            InhibitMode::Delay => "delay",
        }
    }
}

impl fmt::Display for InhibitMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// RAII handle on an inhibitor lock. If this is dropped, the lock is released.
#[derive(Debug)]
pub struct InhibitorLock {
    fd: OwnedFd,
}

impl InhibitorLock {
    pub(crate) fn new(fd: OwnedFd) -> InhibitorLock {
        InhibitorLock { fd }
    }

    // Note: OwnedFd's clone() panics on error, so avoid using it here

    /// Creates a duplicate of the file descriptor backing this inhibitor lock. The caller is responsible
    /// for ensuring that the returned file descriptor is eventually closed.
    pub fn dup_fd(&self) -> Result<RawFd, LogindError> {
        unistd::dup(self.fd.as_raw_fd()).map_err(|err| {
            LogindError::inhibitor_file_error("Duplicating inhibitor lock file descriptor failed".to_string(), err)
        })
    }

    pub fn release(self) -> Result<(), LogindError> {
        unistd::close(self.fd.into_fd()).map_err(|err| {
            LogindError::inhibitor_file_error("Could not release inhibitor lock".to_string(), err)
        })
    }
}

impl IntoRawFd for InhibitorLock {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_fd()
    }
}

impl fmt::Display for InhibitorLock {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.fd.as_raw_fd())
    }
}
