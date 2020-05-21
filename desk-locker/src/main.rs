use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result as AnyResult;
use dbus::blocking::Connection;
use log::{debug, error, info};

use desk_logind::{SessionId, Logind, session_id};
use desk_logind::inhibitor::{InhibitEvent, InhibitEventSet, InhibitMode, InhibitorLock};

static INHIBITOR_WHO: &str = "desk-locker";
static INHIBITOR_WHY: &str = "Lock screen on sleep";

struct Locker {
    // Configuration
    pass_inhibitor: bool,
    set_idle: bool,

    session_id: SessionId,

    // State
    inhibitor_lock: Option<InhibitorLock>,
    locker_process: Option<Child>
}

// Locker flow:
//
// On startup:
//    - take an inhibitor lock
//
// On sleep:
//    - Start locker
//    - Release the inhibitor lock
//
// On resume:
//    - Take a new inhibitor lock
//    - Note: if locker was already running, no need to restart it with the new inhibitor lock
//
// On lock:
//    - Start locker
//
// On unlock:
//    - Kill locker, if running
//
// To start the locker:
//    - Check if we already spawned a running locker, if so return
//    - Otherwise, start a new locker
//    - Depending on configuration, pass the new locker a duplicate of our inhibitor lock
//
// Note: xss-lock _only_ passes the inhibitor lock FD before sleeping. However, I think always passing
// it should be fine, since the locker receives a duplicate and should close it when ready anyways.
// This is simpler to reason about and avoids a potential race condition where the system suspends
// while the locker is starting in response to a `Lock` signal.

impl Locker {
    fn new(logind: &Logind, pass_inhibitor: bool, set_idle: bool) -> AnyResult<Locker> {
        let session_id = session_id()?;
        let inhibitor_lock = Locker::take_lock(logind)?;
        Ok(Locker {
            pass_inhibitor,
            set_idle,
            session_id,
            inhibitor_lock: Some(inhibitor_lock),
            locker_process: None
        })
    }

    /// Helper to take out a new inhibitor lock. Called at startup and on when resuming from sleep.
    fn take_lock(logind: &Logind<'_>) -> AnyResult<InhibitorLock> {
        let events = InhibitEventSet::with_event(InhibitEvent::Sleep);
        let lock = logind.inhibit(INHIBITOR_WHO, INHIBITOR_WHY, &events, InhibitMode::Delay)?;
        debug!("Took inhibitor lock {}", lock);
        Ok(lock)
    }

    /// Get a reference to the inhibitor lock. Panics if the lock is not held.
    fn inhibitor_lock(&self) -> &InhibitorLock {
        match self.inhibitor_lock {
            Some(ref lock) => lock,
            None => panic!("No inhibitor lock held")
        }
    }

    /// Releases the inhibitor lock, if held.
    fn release_lock(&mut self) -> AnyResult<()> {
        if let Some(lock) = self.inhibitor_lock.take() {
            debug!("Releasing inhibitor lock {}", lock);
            lock.release()?;
        }
        Ok(())
    }

    /// Helper to start a new locker process.
    fn spawn_locker(&mut self) -> AnyResult<Child> {
        debug!("Spawning screen locker");
        let mut cmd = Command::new("xsecurelock");
        if self.pass_inhibitor {
            let inhibitor = self.inhibitor_lock().dup_fd()?;
            cmd.env("XSS_SLEEP_LOCK_FD", inhibitor.to_string());
        }
        Ok(cmd.spawn()?)
    }

    /// Starts a new screen locker process, if one isn't already running.
    fn start_locker(&mut self) -> AnyResult<()> {
        // If there's already a locker, make sure it didn't die
        if let Some(ref mut locker) = self.locker_process {
            // If try_wait returns None, then the locker is still running. However, we have no
            // guarantee that it won't crash immediately after.
            if let None = locker.try_wait()? {
                debug!("Screen locker is already running, will not restart");
                return Ok(());
            }
        }

        self.locker_process = Some(self.spawn_locker()?);
        Ok(())
    }

    /// Kill the locker process, if running.
    fn kill_locker(&mut self) -> AnyResult<()> {
        if let Some(mut locker) = self.locker_process.take() {
            debug!("Killing screen locker {:?}", locker);
            locker.kill()?;
        }
        Ok(())
    }

    fn on_sleep(&mut self) -> AnyResult<()> {
        self.start_locker()?;
        self.release_lock()?;
        Ok(())
    }

    fn on_resume(&mut self, logind: &Logind) -> AnyResult<()> {
        self.inhibitor_lock = Some(Locker::take_lock(logind)?);
        Ok(())
    }

    fn on_lock(&mut self, logind: &Logind) -> AnyResult<()> {
        self.start_locker()?;
        if self.set_idle {
            let session = logind.session(&self.session_id)?;
            session.set_idle_hint(true)?;
        }
        Ok(())
    }

    fn on_unlock(&mut self, logind: &Logind) -> AnyResult<()> {
        self.kill_locker()?;
        if self.set_idle {
            let session = logind.session(&self.session_id)?;
            session.set_idle_hint(false)?;
        }
        Ok(())
    }
}

fn run() -> AnyResult<()> {
    let mut conn = Connection::new_system()?;

    let logind = Logind::new(&conn);
    let locker = Arc::new(Mutex::new(Locker::new(&logind, true, true)?));

    // Set up session lock/unlock callbacks
    let session = logind.current_session()?;

    {
        let locker = locker.clone();
        session.on_lock(move |logind| {
            if let Err(e) = locker.lock().unwrap().on_lock(&logind) {
                error!("Handling lock failed: {}", e);
            }
        })?;
    }

    {
        let locker = locker.clone();
        session.on_unlock(move |logind| {
            if let Err(e) = locker.lock().unwrap().on_unlock(&logind) {
                error!("Handling unlock failed: {}", e);
            }
        })?;
    }

    // Then set up sleep/resume callbacks
    let sleep_locker = locker.clone();
    let resume_locker = locker.clone();
    logind.on_sleep(
        move |_logind| {
            if let Err(e) = sleep_locker.lock().unwrap().on_sleep() {
                error!("Handling sleep failed: {}", e);
            }
        },
        move |logind| {
            if let Err(e) = resume_locker.lock().unwrap().on_resume(&logind) {
                error!("Handling resume failed: {}", e);
            }
        }
    )?;

    info!("Waiting for lock events...");
    loop {
        if let Err(e) = conn.process(Duration::from_secs(5)) {
            error!("Processing D-Bus events failed: {}", e);
        }
    }
}

pub fn main() {
    env_logger::init();
    if let Err(e) = run() {
        error!("{}", e);
    }
}
