use anyhow::Context;
use dbus::blocking::Connection;
use desk_logind::Logind;
use gdk::enums::key::{self, Key};

/// Action to show in the exit screen
pub struct Action {
    /// Key to trigger this action on press
    key: Key,

    /// Name of the icon to display in the button for this action
    icon: &'static str,

    run: fn() -> anyhow::Result<()>,
}

impl Action {
    pub const fn new(key: Key, icon: &'static str, run: fn() -> anyhow::Result<()>) -> Action {
        Action { key, icon, run }
    }

    pub fn key(&self) -> Key {
        self.key
    }

    pub fn icon(&self) -> &str {
        self.icon
    }

    pub fn run(&self) -> anyhow::Result<()> {
        (self.run)()
    }

    pub fn run_fn(&self) -> fn() -> anyhow::Result<()> {
        self.run
    }
}

pub const ACTIONS: &'static [Action] = &[
    Action::new(key::l, "system-lock-screen", lock),
    Action::new(key::s, "system-suspend", suspend),
];

fn suspend() -> anyhow::Result<()> {
    let conn = Connection::new_system().context("Could not connect to D-Bus")?;
    let logind = Logind::new(&conn);
    logind.suspend(true).context("Error suspending system")?;
    Ok(())
}

fn lock() -> anyhow::Result<()> {
    let conn = Connection::new_system().context("Could not connect to D-Bus")?;
    let logind = Logind::new(&conn);
    let session = logind
        .current_session()
        .context("Could not get current logind session")?;
    session.lock().context("Error locking session")?;
    Ok(())
}
