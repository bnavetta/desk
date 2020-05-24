//! Core locker implementation.

use std::process::{Child, Command};

use anyhow::{bail, Context, Result as AnyResult};
use log::{info, debug};

use desk_logind::inhibitor::{InhibitEvent, InhibitEventSet, InhibitMode, InhibitorLock};
use desk_logind::{Logind, SessionId};

static INHIBITOR_WHO: &str = "desk-locker";
static INHIBITOR_WHY: &str = "Lock screen on sleep";

pub struct Locker {
    pass_inhibitor_fd: bool,
    manage_idle_hint: bool,
    locker_command: Vec<String>,

    session_id: SessionId,
    inhibitor_lock: Option<InhibitorLock>,
    locker_process: Option<Child>,
}

/// Screen locker implementation.
///
/// The locker assumes it _always_ holds a delay inhibitor lock for sleep events. It takes the lock
/// on creation and only releases it right before the system sleeps. On resuming from sleep, it
/// reacquires the lock as soon as possible.
///
/// This means that, when using `pass_inhibitor`, the child screen locker process is passed a duplicate
/// of the inhibitor lock file descriptor. The child process is also _always_ passed an inhibitor lock,
/// not only when the system is about to sleep. This is a difference from `xss-lock`.
impl Locker {
    /// Creates a new locker. The locker will immediately take a sleep inhibitor lock and determine
    /// some needed session information.
    ///
    /// # Errors
    /// If unable to determine the session ID or take an inhibitor lock, returns a logind error.
    /// If the locker command is empty, returns an error message
    ///
    pub fn new(
        logind: &Logind,
        pass_inhibitor_fd: bool,
        manage_idle_hint: bool,
        locker_command: Vec<String>,
    ) -> AnyResult<Locker> {
        if locker_command.is_empty() {
            bail!("Locker command not provided");
        }

        let session_id = desk_logind::session_id()?;
        let inhibitor_lock = Locker::take_lock(logind)?;
        Ok(Locker {
            pass_inhibitor_fd,
            manage_idle_hint,
            locker_command,
            session_id,
            inhibitor_lock: Some(inhibitor_lock),
            locker_process: None,
        })
    }

    /// Helper to take out a new inhibitor lock. Called at startup and on when resuming from sleep.
    fn take_lock(logind: &Logind<'_>) -> AnyResult<InhibitorLock> {
        let events = InhibitEventSet::with_event(InhibitEvent::Sleep);
        let lock = logind
            .inhibit(INHIBITOR_WHO, INHIBITOR_WHY, &events, InhibitMode::Delay)
            .context("Taking sleep lock failed")?;
        debug!("Took inhibitor lock {}", lock);
        Ok(lock)
    }

    /// Get a reference to the inhibitor lock. Panics if the lock is not held.
    fn inhibitor_lock(&self) -> &InhibitorLock {
        match self.inhibitor_lock {
            Some(ref lock) => lock,
            None => panic!("No inhibitor lock held"),
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

        debug!("Running screen locker {:?}", self.locker_command);
        let mut cmd = Command::new(&self.locker_command[0]);
        self.locker_command.iter().skip(1).for_each(|a| {
            cmd.arg(a);
        });
        if self.pass_inhibitor_fd {
            let inhibitor = self.inhibitor_lock().dup_fd()?;
            cmd.env("XSS_SLEEP_LOCK_FD", inhibitor.to_string());
        }
        let process = cmd.spawn()?;
        debug!("Started screen locker with pid {}", process.id());
        self.locker_process = Some(process);

        Ok(())
    }

    /// Kill the locker process, if running.
    fn kill_locker(&mut self) -> AnyResult<()> {
        if let Some(mut locker) = self.locker_process.take() {
            debug!("Killing screen locker with pid {}", locker.id());
            locker.kill().context("Could not kill locker")?;
        }
        Ok(())
    }

    /// If we're managing the idle hint, set it to true
    fn set_idle(&self, logind: &Logind) -> AnyResult<()> {
        if self.manage_idle_hint {
            debug!("Setting idle hint");
            let session = logind.session(&self.session_id)?;
            session.set_idle_hint(true)?;
        }
        Ok(())
    }

    /// If we're managing the idle hint, set it to false
    fn clear_idle(&self, logind: &Logind) -> AnyResult<()> {
        if self.manage_idle_hint {
            debug!("Clearing idle hint");
            let session = logind.session(&self.session_id)?;
            session.set_idle_hint(false)?;
        }
        Ok(())
    }

    /// Called when the system is about to sleep. This starts the screen locker if it's not
    /// already running and releases the inhibitor lock.
    pub fn on_sleep(&mut self) -> AnyResult<()> {
        info!("Preparing for system sleep");
        self.start_locker()
            .context("Could not start locker before sleeping")?;
        self.release_lock()
            .context("Could not release inhibitor lock, sleep may be delayed")?;
        Ok(())
    }

    /// Called when the system has resumed from sleep. This acquires a new inhibitor lock.
    pub fn on_resume(&mut self, logind: &Logind) -> AnyResult<()> {
        info!("Resumed from system sleep");
        self.inhibitor_lock = Some(Locker::take_lock(logind)?);
        Ok(())
    }

    /// Lock the screen. This will start the screen locker if it's not already running and, if
    /// configured with `manage_idle_hint`, set the session's idle hint to `true`.
    pub fn lock(&mut self, logind: &Logind) -> AnyResult<()> {
        info!("Locking screen...");
        self.start_locker()?;
        self.set_idle(logind)?;
        Ok(())
    }

    /// Unlock the screen. This will kill the screen locker if it's running and, if configured with
    /// `manage_idle_hint`, set the session's idle hint to false.
    pub fn unlock(&mut self, logind: &Logind) -> AnyResult<()> {
        info!("Unlocking screen...");
        self.kill_locker()?;
        self.clear_idle(logind)?;
        Ok(())
    }

    /// Called periodically to reap the screen locker process.
    pub fn poll_locker(&mut self, logind: &Logind) -> AnyResult<()> {
        if let Some(ref mut locker) = self.locker_process {
            if let Some(status) = locker.try_wait()? {
                debug!("Screen locker exited with {}", status);
                self.clear_idle(logind)?;
                self.locker_process = None;
            }
        }

        Ok(())
    }
}
