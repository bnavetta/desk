use anyhow::{anyhow, bail, Context, Result as AnyResult};
use xcb;
use xcb::screensaver;

/// Client for the [X11 screen saver extension](https://www.x.org/releases/X11R7.7/doc/scrnsaverproto/saver.html).
/// For now, only supports listening for screen saver events.
pub struct ScreenSaver {
    conn: xcb::Connection,
    notify_event: u8,
}

impl ScreenSaver {
    pub fn new() -> AnyResult<ScreenSaver> {
        let (conn, screen_num) =
            xcb::Connection::connect(None).context("Could not connect to X server")?;

        let ext_data = conn
            .get_extension_data(screensaver::id())
            .context("Could not get X Screen Saver extension data")?;
        if !ext_data.present() {
            bail!("X Screen Saver extension not present");
        }

        // Figure out the actual X11 event response type we'll see
        let notify_event = ext_data.first_event() + screensaver::NOTIFY;

        // TODO: check extension protocol version

        // TODO: multi-monitor support (can watch for screens being added/removed)
        let setup = conn.get_setup();
        let screen = setup
            .roots()
            .nth(screen_num as usize)
            .ok_or(anyhow!("Could not get X11 screen {}", screen_num))?;

        screensaver::select_input_checked(
            &conn,
            screen.root(),
            screensaver::EVENT_NOTIFY_MASK | screensaver::EVENT_CYCLE_MASK,
        )
        .request_check()
        .context(anyhow!(
            "Could not subscribe to X11 screen saver events on screen {}",
            screen_num
        ))?;

        Ok(ScreenSaver { conn, notify_event })
    }

    pub fn poll_event(&self) -> Option<ScreenSaverEvent> {
        self.conn.poll_for_event().and_then(|event| {
            // Don't know why this is needed, but _every_ XCB example I've seen does it
            let event_type = event.response_type() & !0x80;
            if event_type == self.notify_event {
                // Safety: verified above that this is a NotifyEvent, according to the event type from the extension data
                let event: &xcb::screensaver::NotifyEvent = unsafe { xcb::cast_event(&event) };

                match event.state() as u32 {
                    screensaver::STATE_OFF => Some(ScreenSaverEvent::Off),
                    screensaver::STATE_ON => Some(ScreenSaverEvent::On),
                    screensaver::STATE_CYCLE => Some(ScreenSaverEvent::Cycle),
                    screensaver::STATE_DISABLED => Some(ScreenSaverEvent::Disabled),
                    _ => None,
                }
            } else {
                None
            }
        })
    }
}

/// Events produced by X11 on screen saver state changes.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum ScreenSaverEvent {
    /// The screen saver turned on
    On,

    /// The screen saver turned off
    Off,

    /// The screen saver cycled to a new image
    Cycle,

    /// The screen saver was disabled
    Disabled,
}
