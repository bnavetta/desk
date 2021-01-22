use std::collections::HashMap;
use std::env;
use std::process::Command;

use anyhow::{anyhow, Context};
use dbus::blocking::Connection;
use gdk::keys::{constants as keys, Key};
use gdk::keyval_from_name;
use glib::translate::from_glib;

use desk_logind::Logind;

use crate::config::{Config, CustomAction};

/// Action to show in the exit screen
pub struct Action {
    key: Key,
    icon: String,
    description: String,
    run: Box<dyn Fn() -> anyhow::Result<()>>,
}

impl Action {
    /// Icon displayed for this action as a button in the exit screen
    pub fn icon(&self) -> &str {
        &self.icon
    }

    /// A description of what this action does
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Run this action
    pub fn run(&self) -> anyhow::Result<()> {
        (self.run)()
    }
}

pub struct Actions {
    actions: HashMap<String, Action>,
    order: Vec<String>,
}

impl Actions {
    /// Iterate over all actions in the configured display order
    pub fn iter(&self) -> impl Iterator<Item = (&'_ String, &'_ Action)> {
        self.order
            .iter()
            .flat_map(move |act| Some(act).zip(self.actions.get(act)))
    }

    /// Find the action with the given keyboard shortcut, if one is defined
    pub fn find_by_key(&self, key: Key) -> Option<&Action> {
        self.actions.values().find(|act| act.key == key)
    }

    pub fn get(&self, name: &str) -> &Action {
        &self.actions[name]
    }
}

pub fn build_actions(config: Config) -> Actions {
    // First, add built-in actions
    let mut actions = HashMap::new();
    actions.insert(
        "lock".to_string(),
        static_action(keys::l, "system-lock-screen", "Lock your screen", lock),
    );
    actions.insert(
        "suspend".to_string(),
        static_action(
            keys::s,
            "system-suspend",
            "Put the computer to sleep",
            suspend,
        ),
    );
    actions.insert(
        "hibernate".to_string(),
        static_action(
            keys::h,
            "system-hibernate",
            "Hibernate the computer",
            hibernate,
        ),
    );
    actions.insert(
        "reboot".to_string(),
        static_action(keys::r, "system-restart", "Restart the computer", restart),
    );
    actions.insert(
        "shutdown".to_string(),
        static_action(
            keys::p,
            "system-shutdown",
            "Shut the computer off",
            shut_down,
        ),
    );

    // Destructure so we can use strings from the config instead of copying
    let Config {
        quit_command,
        order,
        actions: custom_actions,
    } = config;

    if let Some(quit_command) = quit_command {
        actions.insert(
            "quit".to_string(),
            Action {
                key: keys::q,
                icon: "system-log-out".to_string(),
                description: "Log out".to_string(),
                run: exec_action(quit_command),
            },
        );
    }

    for (name, custom) in custom_actions.into_iter() {
        let key = from_glib(keyval_from_name(&custom.key));
        let CustomAction {
            icon,
            description,
            command,
            ..
        } = custom;
        actions.insert(
            name,
            Action {
                key,
                icon,
                description,
                run: exec_action(command),
            },
        );
    }

    Actions { actions, order }
}

/// Helper for defining built-in actions
fn static_action(
    key: Key,
    icon: &str,
    description: &str,
    run: fn() -> anyhow::Result<()>,
) -> Action {
    Action {
        key,
        icon: icon.to_string(),
        description: description.to_string(),
        run: Box::new(run),
    }
}

/// Helper that creates an action function to run the given command in the user's default shell
fn exec_action(command: String) -> Box<dyn Fn() -> anyhow::Result<()>> {
    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());

    Box::new(move || {
        let status = Command::new(&shell)
            .arg("-c")
            .arg(&command)
            .status()
            .with_context(|| format!("Could not execute {} (via {})", command, shell))?;
        if !status.success() {
            Err(anyhow!(
                "Command {} (via {}) failed: {}",
                command,
                shell,
                status
            ))
        } else {
            Ok(())
        }
    })
}

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
