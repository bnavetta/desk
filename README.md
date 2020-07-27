# Desktop Environment Support Kit

Desk is a set of utilities for doing desktop environment-like things with lightweight window managers. So far it has two, a screen locker helper and
an exit screen.

## `desk-locker`

`desk-locker` is a screen locking utility along the same lines as `xss-lock`. It listens for X screen saver events,
system sleeps, and session lock/unlock events and runs a locker program like `xsecurelock` or `i3lock`. Unlike
`xss-lock`, it uses the `XDG_SESSION_ID` environment variable to determine which session to lock, so it can be run
from a systemd user unit.

### Usage

```shell script
# Basic usage: run xsecurelock whenever the screen should be locked
$ desk-locker xsecurelock

# Pass a logind inhibitor lock to xsecurelock. This prevents the system from sleeping until xsecurelock reports that
# it's ready. `--pass-inhibitor-lock` should work with any screen locker that supports xss-lock's `--transfer-sleep-lock` flag.
$ desk-locker --pass-inhibitor-lock xsecurelock

# Additionally, update the session's idle hint. Logind can be configured to do something (ex. put the system to sleep)
# after all sessions have been idle for a certain amount of time. If you don't already have something that updates the
# idle hint, setting it whenever the screen locker is active is a reasonable default.
$ desk-locker --set-idle-hint --pass-inhibitor-lock xsecurelock
```

## `desk-exit-screen`

`desk-exit-screen` shows a GUI for exiting your window manager (shutting down, rebooting, logging out, etc.).

It supports custom action using a configuration file in `~/.config/desk/exit-screen.toml`:

```toml
# Add an action for quitting / logging out, by running the given shell command
# For example, i3-msg exit
quit_command = "<window-manager-specific quit command>"

# Customize the order actions are shown in. This must be set to display any custom actions.
# By default, the order is
# [ "lock", "quit", "suspend", "hibernate", "reboot", "shutdown" ]
order = ["act1", "act2", "..."]

# Use more [actions.<action name>] sections to define additional actions
[actions.mycustomaction]
key = "u" # Keyboard shortcut for this action
icon = "system-upgrade" # GTK+ icon name to use for the button
description = "Upgrade the system" # Text description of the action
command = "yay" # Command to run
```

## `desk-logind`

This is a Rust library for using the `logind` [D-Bus API](https://www.freedesktop.org/wiki/Software/systemd/logind/).

