use std::env;
use std::process::Command;

use anyhow::{anyhow, Context};
use dbus::blocking::Connection;
use desk_logind::Logind;
use gdk::keys::{Key, constants as keys};

/// Since quitting is window-manager-specific, we run a user-provided command from this environment
/// variable instead of trying to do it ourselves.
///
/// It might be worth adding support for specific window managers at some point
/// (for example, if using i3, run `i3-msg exit`)
const QUIT_COMMAND_VAR: &str = "DESK_QUIT";

/// Action to show in the exit screen
pub struct Action {
    key: Key,
    icon: &'static str,
    description: &'static str,
    run: fn() -> anyhow::Result<()>,
}

impl Action {
    /// Key to trigger this action on press
    pub fn key(&self) -> Key {
        self.key.clone()
    }

    /// Icon displayed for this action as a button in the exit screen
    pub fn icon(&self) -> &str {
        self.icon
    }

    /// A description of what this action does
    pub fn description(&self) -> &str {
        self.description
    }

    /// Run this action
    pub fn run(&self) -> anyhow::Result<()> {
        (self.run)()
    }

    /// Function pointer for running this action
    pub fn run_fn(&self) -> fn() -> anyhow::Result<()> {
        self.run
    }
}

pub const ACTIONS: &'static [Action] = &[
    // Order in this array corresponds to order on the screen. Actions are roughly ordered from
    // most-disruptive to least-disruptive
    Action {
        key: keys::l,
        icon: "system-lock-screen",
        description: "Lock your screen",
        run: lock,
    },
    Action {
        key: keys::q,
        icon: "system-log-out",
        description: "Log out",
        run: quit,
    },
    Action {
        key: keys::s,
        icon: "system-suspend",
        description: "Put the computer to sleep",
        run: suspend,
    },
    Action {
        key: keys::h,
        icon: "system-hibernate",
        description: "Hibernate the computer",
        run: hibernate,
    },
    // TODO: suspend-then-hibernate as well?
    Action {
        key: keys::r,
        icon: "system-restart",
        description: "Restart the computer",
        run: restart,
    },
    Action {
        key: keys::p,
        icon: "system-shutdown",
        description: "Shut the computer off",
        run: shut_down,
    },
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

fn quit() -> anyhow::Result<()> {
    let command = env::var(QUIT_COMMAND_VAR).with_context(|| {
        format!(
            "Could not figure out how to quit. Is {} set?",
            QUIT_COMMAND_VAR
        )
    })?;

    let status = Command::new("/bin/bash")
        .arg("-c")
        .arg(&command)
        .status()
        .with_context(|| format!("Could not start quit command `{}` via bash", command))?;
    if !status.success() {
        Err(anyhow!("Quit command `{}` failed: {}", command, status))
    } else {
        Ok(())
    }
}

fn hibernate() -> anyhow::Result<()> {
    let conn = Connection::new_system().context("Could not connect to D-Bus")?;
    let logind = Logind::new(&conn);
    logind.hibernate(true).context("Error hibernating system")?;
    Ok(())
}

fn restart() -> anyhow::Result<()> {
    let conn = Connection::new_system().context("Could not connect to D-Bus")?;
    let logind = Logind::new(&conn);
    logind.reboot(true).context("Error rebooting system")?;
    Ok(())
}

fn shut_down() -> anyhow::Result<()> {
    let conn = Connection::new_system().context("Could not connect to D-Bus")?;
    let logind = Logind::new(&conn);
    logind
        .power_off(true)
        .context("Error shutting down system")?;
    Ok(())
}
