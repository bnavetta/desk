//! `systemd-logind` client library
#![feature(backtrace)]
use std::env;
use std::time::Duration;

use dbus::blocking::{Connection, Proxy};
use dbus::Message;

use crate::api::manager::{
    OrgFreedesktopLogin1Manager, OrgFreedesktopLogin1ManagerPrepareForSleep,
};
use crate::inhibitor::{InhibitorLock, InhibitMode, InhibitEventSet};
pub use crate::error::LogindError;
pub use crate::session::{SessionId, Session};

mod api;
mod error;
mod session;
pub mod inhibitor;

pub fn session_id() -> Result<SessionId, LogindError> {
    match env::var("XDG_SESSION_ID") {
        Ok(id) => Ok(SessionId::new(id)),
        Err(_) => Err(LogindError::no_session_id()),
    }
}

/// A logind client connection. This is a relatively thin wrapper over the
/// [D-Bus API](https://www.freedesktop.org/wiki/Software/systemd/logind/).
pub struct Logind<'a> {
    conn: &'a Connection,
    timeout: Duration,
}

impl <'a> Logind <'a> {
    pub fn new(conn: &'a Connection) -> Logind {
        Logind {
            conn,
            timeout: Duration::from_millis(500),
        }
    }

    /// Get a handle to a logind session by ID.
    pub fn session(&self, id: &SessionId) -> Result<Session<'a>, LogindError> {
        let manager = self.manager();
        let path = manager.get_session(id.as_str())?;
        let proxy = Proxy::new("org.freedesktop.login1", path, self.timeout, self.conn.clone());
        Ok(Session::new(proxy))
    }

    /// Get a handle to the current logind session.
    pub fn current_session(&self) -> Result<Session<'a>, LogindError> {
        let id = session_id()?;
        self.session(&id)
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
        Ok(InhibitorLock::new(fd))
    }

    pub fn on_sleep<F: Fn(Logind) -> () + Send + 'static, G: Fn(Logind) -> () + Send + 'static>(
        &self,
        pre_sleep: F,
        post_sleep: G,
    ) -> Result<(), LogindError> {
        let manager = self.manager();
        match manager.match_signal(
            move |signal: OrgFreedesktopLogin1ManagerPrepareForSleep,
                  conn: &Connection,
                  _: &Message| {
                if signal.arg0 {
                    pre_sleep(Logind::new(conn));
                } else {
                    post_sleep(Logind::new(conn));
                }
                true
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(LogindError::match_failed("PrepareForSleep", e)),
        }
    }

    fn manager(&self) -> Proxy<'_, &'a Connection> {
        Proxy::new(
            "org.freedesktop.login1",
            "/org/freedesktop/login1",
            self.timeout,
            self.conn,
        )
    }
}

