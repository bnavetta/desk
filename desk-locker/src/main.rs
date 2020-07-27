use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result as AnyResult;
use dbus::blocking::Connection;
use env_logger::Env;
use log::{error, info};
use structopt::StructOpt;

use desk_logind::Logind;

use crate::locker::Locker;
use crate::screensaver::{ScreenSaver, ScreenSaverEvent};

mod locker;
mod screensaver;

#[derive(StructOpt)]
struct Args {
    /// Pass file descriptor for a sleep inhibitor lock to screen locker process.
    ///
    /// This uses the `xss-lock` protocol of passing the file descriptor number in the `XSS_SLEEP_LOCK_FD`
    /// environment variable. Unlike `xss-lock`, the screen locker is always passed a file descriptor,
    /// not just if the system is about to go to sleep. The screen locker should close this file
    /// descriptor once it is ready and has locked the screen.
    #[structopt(long, short = "l")]
    pass_inhibitor_lock: bool,

    /// Manage the session idle hint.
    ///
    /// If this is set, then the session will be marked as idle when locking the screen and marked
    /// as not idle when unlocking it. The idle hint is not updated when the system goes to or
    /// resumes from sleep.
    #[structopt(long, short = "i")]
    set_idle_hint: bool,

    /// Screen locker command to run, such as `xsecurelock` or `i3lock`. This command should not
    /// fork.
    #[structopt(required = true)]
    locker: Vec<String>,
}

fn run(args: Args) -> AnyResult<()> {
    let screen_saver = ScreenSaver::new()?;

    let conn = Connection::new_system()?;

    let logind = Logind::new(&conn);
    let locker = Arc::new(Mutex::new(Locker::new(
        &logind,
        args.pass_inhibitor_lock,
        args.set_idle_hint,
        args.locker,
    )?));

    // Set up session lock/unlock callbacks
    let session = logind.current_session()?;

    {
        let locker = locker.clone();
        session.on_lock(move |logind| {
            if let Err(e) = locker.lock().unwrap().lock(&logind) {
                error!("Handling lock failed: {}", e);
            }
        })?;
    }

    {
        let locker = locker.clone();
        session.on_unlock(move |logind| {
            if let Err(e) = locker.lock().unwrap().unlock(&logind) {
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
        },
    )?;

    info!("Waiting for events...");
    loop {
        if let Err(e) = conn.process(Duration::from_millis(100)) {
            error!("Processing D-Bus events failed: {}", e);
        }

        // Must not hold lock while calling conn.process - since the logind signal callbacks also
        // use the locker, this can deadlock
        let mut locker = locker.lock().unwrap();
        locker.poll_locker(&Logind::new(&conn))?;

        if let Some(event) = screen_saver.poll_event() {
            let logind = Logind::new(&conn);
            match event {
                ScreenSaverEvent::On | ScreenSaverEvent::Cycle => locker.lock(&logind)?,
                // Do not unlock when the screen saver deactivates - that defeats the point of having this :P
                _ => (),
            }
        }
    }
}

pub fn main() {
    let env = Env::new()
        .filter_or("DESK_LOG", "info")
        .write_style("DESK_LOG_STYLE");
    env_logger::init_from_env(env);

    let args = Args::from_args();
    if let Err(e) = run(args) {
        error!("{}", e);
    }
}
