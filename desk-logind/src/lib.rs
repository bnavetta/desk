//! `systemd-logind` client library
#![feature(backtrace)]
use std::env;
use std::fmt;
use std::os::unix::io::{IntoRawFd, AsRawFd, RawFd};
use std::sync::Arc;
use std::time::Duration;

use dbus::arg::OwnedFd;
use dbus::blocking::{Connection, Proxy};
use dbus::Message;
use nix::unistd;

use crate::api::manager::{
    OrgFreedesktopLogin1Manager, OrgFreedesktopLogin1ManagerPrepareForSleep,
};
use crate::api::session::{
    OrgFreedesktopLogin1Session, OrgFreedesktopLogin1SessionLock, OrgFreedesktopLogin1SessionUnlock,
};
pub use crate::error::LogindError;

mod api;
mod error;

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub fn session_id() -> Result<SessionId, LogindError> {
    match env::var("XDG_SESSION_ID") {
        Ok(id) => Ok(SessionId(id)),
        Err(_) => Err(LogindError::no_session_id()),
    }
}

/// A logind client connection. This is a relatively thin wrapper over the
/// [D-Bus API](https://www.freedesktop.org/wiki/Software/systemd/logind/).
pub struct Logind {
    conn: Arc<Connection>,
    timeout: Duration,
}

impl Logind {
    pub fn new(conn: Arc<Connection>) -> Logind {
        Logind {
            conn,
            timeout: Duration::from_millis(500),
        }
    }

    /// Get a handle to a logind session by ID.
    pub fn session(&self, id: SessionId) -> Result<Session, LogindError> {
        let manager = self.manager();
        let path = manager.get_session(id.as_str())?;
        let proxy = Proxy::new("org.freedesktop.login1", path, self.timeout, self.conn.clone());
        Ok(Session { id, proxy })
    }

    /// Get a handle to the current logind session.
    pub fn current_session(&self) -> Result<Session, LogindError> {
        let id = session_id()?;
        self.session(id)
    }

    pub fn inhibit(
        &self,
        who: &str,
        why: &str,
        events: &InhibitEventSet,
        mode: InhibitMode,
    ) -> Result<InhibitorLock, LogindError> {
        let manager = self.manager();
        let fd = manager.inhibit(events.as_str(), who, why, mode.as_str())?;
        Ok(InhibitorLock { fd })
    }

    pub fn on_sleep<F: Fn() -> () + Send + 'static, G: Fn() -> () + Send + 'static>(
        &self,
        pre_sleep: F,
        post_sleep: G,
    ) -> Result<(), LogindError> {
        let manager = self.manager();
        match manager.match_signal(
            move |signal: OrgFreedesktopLogin1ManagerPrepareForSleep,
                  _: &Connection,
                  _: &Message| {
                if signal.arg0 {
                    pre_sleep();
                } else {
                    post_sleep();
                }
                true
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(LogindError::match_failed("PrepareForSleep", e)),
        }
    }

    /// Wait for any events subscribed to on the login manager or any sessions
    pub fn await_events(&mut self, timeout: Duration) -> Result<(), LogindError> {
        self.conn.process(timeout)?;
        Ok(())
    }

    fn manager(&self) -> Proxy<'_, Arc<Connection>> {
        Proxy::new(
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            self.timeout,
            self.conn.clone(),
        )
    }
}

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
    pub fn new() -> InhibitEventSet {
        InhibitEventSet(String::new())
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
#[derive(Debug, Clone)]
pub struct InhibitorLock {
    fd: OwnedFd,
}

impl InhibitorLock {
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

/// Handle to a logind session
pub struct Session {
    id: SessionId,
    proxy: Proxy<'static, Arc<Connection>>,
}

impl Session {
    pub fn name(&self) -> Result<String, LogindError> {
        let name = self.proxy.name()?;
        Ok(name)
    }

    /// Register a callback to run when the session is locked.
    pub fn on_lock<F: Fn() -> () + Send + 'static>(&self, cb: F) -> Result<(), LogindError> {
        match self.proxy.match_signal(
            move |_: OrgFreedesktopLogin1SessionLock, _: &Connection, _: &Message| {
                cb();
                true
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(LogindError::match_failed("Lock", e)),
        }
    }

    pub fn on_unlock<F: Fn() -> () + Send + 'static>(&self, cb: F) -> Result<(), LogindError> {
        match self.proxy.match_signal(
            move |_: OrgFreedesktopLogin1SessionUnlock, _: &Connection, _: &Message| {
                cb();
                true
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(LogindError::match_failed("Unlock", e)),
        }
    }
}

impl<'a> fmt::Debug for Session<'a> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Session").field("id", &self.id).finish()
    }
}
