#![feature(const_fn)]

use std::env;

use atk::prelude::*;
use env_logger::Env;
use gdk::enums::key;
use gdk::{Screen, WindowTypeHint};
use gio::prelude::*;
use gtk::prelude::*;
use gtk::{
    Application, Button, CssProvider, IconLookupFlags, IconTheme, Image, Orientation, StyleContext,
    Window, WindowType,
};
use log::error;

use anyhow::{anyhow, Context};

mod actions;

const STYLE: &str = include_str!("desk-exit-screen.css");

const BUTTON_SIZE: i32 = 400;

fn build_ui(app: &Application) -> anyhow::Result<()> {
    let window = Window::new(WindowType::Toplevel);
    app.add_window(&window);
    window.set_widget_name("exit-window"); // used in CSS

    window.connect_key_press_event(|window, event| {
        // Destroy the window (and quit) whenever Escape or a known action key is pressed
        if event.get_keyval() == key::Escape {
            window.destroy();
        } else {
            for action in actions::ACTIONS {
                if event.get_keyval() == action.key() {
                    if let Err(e) = action.run() {
                        error!("Action failed: {}", e);
                    }
                    window.destroy();
                }
            }
        }

        Inhibit(false)
    });

    let container = gtk::Box::new(Orientation::Horizontal, 0);
    container.set_homogeneous(true); // This makes all children the same size

    let icon_theme = IconTheme::get_default().ok_or(anyhow!("No default icon theme"))?;

    for act in actions::ACTIONS {
        let button = create_button(&icon_theme, act.icon())?;
        let run_fn = act.run_fn();
        let window = window.clone(); // Will just increase the reference count
        button.connect_clicked(move |_| {
            if let Err(e) = run_fn() {
                error!("Action failed: {}", e);
            }
            window.destroy();
        });
        if let Some(a11y) = button.get_accessible() {
            a11y.set_description(act.description());
        }
        // TODO: may want to show descriptions on-screen as well
        container.pack_start(&button, false, false, 0);
    }

    // Put the container in more boxes so it doesn't expand
    let vbox = gtk::Box::new(Orientation::Vertical, 0);
    vbox.pack_start(&container, true, false, 0);
    let hbox = gtk::Box::new(Orientation::Horizontal, 0);
    hbox.pack_start(&vbox, true, false, 0);
    window.add(&hbox);

    if let Some(ref screen) = window.get_screen() {
        configure_screen(&window, screen)?;
    }

    window.connect_screen_changed(move |window, screen| {
        if let Some(screen) = screen {
            if let Err(e) = configure_screen(window, screen) {
                error!("Could not adjust to screen change: {}", e);
            }
        }
    });

    window.set_decorated(false);
    window.set_skip_taskbar_hint(true);
    window.set_skip_pager_hint(true);
    window.set_type_hint(WindowTypeHint::Desktop);
    window.set_keep_above(true);
    window.show_all();
    window.stick();
    // window.fullscreen();
    Ok(())
}

/// Creates a new button with the given icon, scaled to `BUTTON_SIZE`.
fn create_button(icon_theme: &IconTheme, icon_name: &str) -> anyhow::Result<Button> {
    // Have to load the icon image directly to make it the right size
    let icon = icon_theme
        .load_icon(icon_name, BUTTON_SIZE, IconLookupFlags::empty())
        .with_context(|| format!("Could not load icon {}", icon_name))?
        .ok_or_else(|| anyhow!("Icon {} not found", icon_name))?
        .copy() // GTK docs say to do this so the rest of the icon theme can be freed if needed
        .ok_or_else(|| anyhow!("Could not copy icon {}", icon_name))?;

    let button = Button::new();
    button.set_size_request(BUTTON_SIZE, BUTTON_SIZE);
    let image = Image::new_from_pixbuf(Some(&icon));
    button.set_image(Some(&image));
    Ok(button)
}

/// Configure a screen for displaying the exit window
fn configure_screen(window: &Window, screen: &Screen) -> anyhow::Result<()> {
    // Updates the window's GDK visual, which is required for transparency to work correctly.
    window.set_visual(screen.get_rgba_visual().as_ref());

    let monitor = screen
        .get_display()
        .get_primary_monitor()
        .ok_or(anyhow!("Primary monitor does not exist"))?;
    let workarea = monitor.get_workarea();
    window.resize(workarea.width, workarea.height);
    // TODO: I'm not sure if it's polybar or i3, but the window is shifted down a couple pixels from
    //       covering the whole screen. This move is a workaround to fix it for now :/
    window.move_(0, -2);

    // Since GTK objects aren't thread-safe, there's no way to have a shared CSS provider
    let provider = CssProvider::new();
    provider
        .load_from_data(STYLE.as_bytes())
        .context("Could not load CSS")?;
    StyleContext::add_provider_for_screen(
        screen,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    Ok(())
}

fn run() -> anyhow::Result<()> {
    let app = Application::new(Some("com.bennavetta.desk.exit-screen"), Default::default())
        .context("Could not create GTK application")?;

    app.connect_activate(|app| {
        if let Err(e) = build_ui(app) {
            error!("Could not create UI: {}", e);
        }
    });

    app.run(&env::args().collect::<Vec<_>>());

    Ok(())
}

fn main() {
    let env = Env::new()
        .filter_or("DESK_LOG", "info")
        .write_style("DESK_LOG_STYLE");
    env_logger::init_from_env(env);

    if let Err(e) = run() {
        eprintln!("{}", e);
    }
}
