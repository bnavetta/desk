use dbus::blocking::{Connection, Proxy};
use dbus::Message;

use crate::api::session::{
    OrgFreedesktopLogin1Session, OrgFreedesktopLogin1SessionLock, OrgFreedesktopLogin1SessionUnlock,
};
use crate::error::LogindError;
use crate::Logind;

#[derive(Debug, Eq, PartialEq, Hash)]
pub struct SessionId(String);

impl SessionId {
    pub fn new(s: String) -> SessionId {
        SessionId(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Handle to a logind session
pub struct Session<'a> {
    proxy: Proxy<'a, &'a Connection>,
}

impl<'a> Session<'a> {
    pub(crate) fn new(proxy: Proxy<'a, &'a Connection>) -> Session<'a> {
        Session { proxy }
    }

    pub fn name(&self) -> Result<String, LogindError> {
        let name = self.proxy.name()?;
        Ok(name)
    }

    pub fn id(&self) -> Result<SessionId, LogindError> {
        let id = self.proxy.id()?;
        Ok(SessionId::new(id))
    }

    /// Register a callback to run when the session is locked.
    pub fn on_lock<F: Fn(Logind) -> () + Send + 'static>(&self, cb: F) -> Result<(), LogindError> {
        match self.proxy.match_signal(
            move |_: OrgFreedesktopLogin1SessionLock, conn: &Connection, _: &Message| {
                cb(Logind::new(conn));
                true
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(LogindError::match_failed("Lock", e)),
        }
    }

    /// Register a callback to run when the session is unlocked.
    pub fn on_unlock<F: Fn(Logind) -> () + Send + 'static>(
        &self,
        cb: F,
    ) -> Result<(), LogindError> {
        match self.proxy.match_signal(
            move |_: OrgFreedesktopLogin1SessionUnlock, conn: &Connection, _: &Message| {
                cb(Logind::new(conn));
                true
            },
        ) {
            Ok(_) => Ok(()),
            Err(e) => Err(LogindError::match_failed("Unlock", e)),
        }
    }

    /// Gets the idle hint for the session.
    pub fn idle_hint(&self) -> Result<bool, LogindError> {
        Ok(self.proxy.idle_hint()?)
    }

    /// Sets the session idle hint.
    pub fn set_idle_hint(&self, idle: bool) -> Result<(), LogindError> {
        self.proxy.set_idle_hint_(idle)?;
        Ok(())
    }
}
