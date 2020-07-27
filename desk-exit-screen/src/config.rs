use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// It might be worth adding support for specific window managers at some point
// (for example, if using i3, infer `i3-msg exit` for `quit_command`)

#[derive(Serialize, Deserialize)]
pub struct Config {
    /// Command to quit the window manager or desktop environment. For example, when using i3, this
    /// would be `i3-msg exit`
    #[serde(default)]
    pub quit_command: Option<String>,

    /// Order to display actions in, by name. Built-in actions are `lock`, `quit`, `suspend`,
    /// `hibernate`, `reboot`, and `shutdown`.
    #[serde(default = "default_action_order")]
    pub order: Vec<String>,

    /// Additional custom actions to display
    #[serde(default)]
    pub actions: HashMap<String, CustomAction>,
}

/// Default action order. Used both when the config file is missing and to provide a default if
/// the config file exists but does not set an order
fn default_action_order() -> Vec<String> {
    vec![
        "lock".to_string(),
        "quit".to_string(),
        "suspend".to_string(),
        "hibernate".to_string(),
        "reboot".to_string(),
        "shutdown".to_string(),
    ]
}

impl Default for Config {
    fn default() -> Self {
        Config {
            quit_command: None,
            order: default_action_order(),
            actions: HashMap::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct CustomAction {
    /// Name of the key that triggers this action
    pub key: String,

    /// Name of the button icon to use
    pub icon: String,

    /// Description of the action, used for accessibility labels
    pub description: String,

    /// Command to run (via shell)
    pub command: String,
}
