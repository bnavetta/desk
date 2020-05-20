use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result as AnyResult;
use log::{info, debug, error};

use desk_logind::{InhibitEvent, InhibitEventSet, InhibitMode, InhibitorLock, Logind, LogindError};

static INHIBITOR_WHO: &str = "desk-locker";
static INHIBITOR_WHY: &str = "Lock screen on sleep";

struct Locker {
    logind: Logind,
    pass_inhibitor: bool,

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
    /// Helper to take out a new inhibitor lock. Called at startup and on when resuming from sleep.
    fn take_lock(&self) -> AnyResult<InhibitorLock> {
        let events = InhibitEventSet::new().add(InhibitEvent::Sleep);
        let lock = self.logind.inhibit(INHIBITOR_WHO, INHIBITOR_WHY, events, InhibitMode::Delay)?;
        debug!("Took inhibitor lock {}", lock);
        Ok(lock)
    }

    /// Ensures an inhibitor lock is held, taking one if necessary. Returns a reference to the lock.
    fn ensure_lock(&mut self) -> AnyResult<&InhibitorLock> {
        // Can't use get_or_insert_with because of possible failure taking the lock
        if let None = self.inhibitor_lock {
            let lock = self.take_lock()?;
            self.inhibitor_lock = Some(lock);
        }

        match self.inhibitor_lock {
            Some(ref lock) => Ok(lock),
            None => unreachable!()
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
            let inhibitor = self.ensure_lock()?.dup_fd()?;
            cmd.env("XSS_SLEEP_LOCK_FD", inhibitor);
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
            debug!("Killing screen locker {}", locker);
            locker.kill()?;
        }
        Ok(())
    }

    fn on_sleep(&mut self) -> AnyResult<()> {
        self.start_locker()?;
        self.release_lock()?;
        Ok(())
    }

    fn on_resume(&mut self) -> AnyResult<()> {
        self.ensure_lock()?;
        Ok(())
    }

    fn on_lock(&mut self) -> AnyResult<()> {
        self.start_locker()?;
        Ok(())
    }

    fn on_unlock(&mut self) -> AnyResult<()> {
        self.kill_locker()?;
        Ok(())
    }

    fn poll(self: Arc<Mutex<Locker>>) -> AnyResult<()> {
        let s = self.lock()?;
    }
}

pub fn main() {
    let logind = Logind::new().unwrap();

    let session = logind.current_session().unwrap();
    println!("Session: {}", session.name().unwrap());

    // This deadlocks
    // instead, move state into one struct w/ a lock around it, callbacks can take lock as needed

    session
        .on_lock(|| {
            println!("Locked!");
        })
        .unwrap();

    session
        .on_unlock(|| {
            println!("Unlocked!");
        })
        .unwrap();

    let mut inhibit_events = InhibitEventSet::new();
    inhibit_events.add(InhibitEvent::Sleep);

    let inhibitor = logind
        .inhibit(
            INHIBITOR_WHO,
            INHIBITOR_WHY,
            &inhibit_events,
            InhibitMode::Delay,
        )
        .unwrap();
    println!("Got inhibitor lock: {:?}", inhibitor);
    let inhibitor = Arc::new(Mutex::new(Some(inhibitor)));
    let inhibitor2 = inhibitor.clone();

    let logind = Arc::new(Mutex::new(logind));
    let logind2 = logind.clone();

    {
        let logind = logind.lock().unwrap();
        logind
            .on_sleep(
                move || {
                    println!("About to sleep!");

                    // Release the inhibitor lock before sleeping
                    let mut inhibitor = inhibitor.lock().unwrap();
                    if let Some(lock) = inhibitor.take() {
                        drop(lock);
                    }
                },
                move || {
                    println!("Resumed from sleep!");

                    let mut inhibitor = inhibitor2.lock().unwrap();
                    let logind = logind2.lock().unwrap();
                    let new_lock = logind
                        .inhibit(
                            INHIBITOR_WHO,
                            INHIBITOR_WHY,
                            &inhibit_events,
                            InhibitMode::Delay,
                        )
                        .unwrap();
                    println!("Took new inhibitor lock: {:?}", new_lock);
                    *inhibitor = Some(new_lock);
                },
            )
            .unwrap();
    }

    loop {
        let mut logind = logind.lock().unwrap();
        logind.await_events(Duration::from_secs(2)).unwrap();
    }
}
