//! Strict EWMH inventory, `XTest` input, and `DestroyNotify` evidence.

use std::sync::mpsc;
use std::time::Instant;

use x11rb::CURRENT_TIME;
use x11rb::connection::Connection as _;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, BUTTON_PRESS_EVENT, BUTTON_RELEASE_EVENT, ChangeWindowAttributesAux,
    ConnectionExt as _, EventMask, GetPropertyReply, KeyButMask, MOTION_NOTIFY_EVENT, Window,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use super::super::assertions::{PhysicalBounds, PhysicalPoint};

const MAX_CLIENT_WINDOWS: usize = 1_024;
const MAX_WINDOW_TITLE_BYTES: usize = 1_024;

pub(super) fn x11_window_is_retired(
    windows: &[WindowRecord],
    child: &WindowRecord,
) -> Result<bool, String> {
    let exact = windows
        .iter()
        .filter(|window| window.window == child.window)
        .count();
    let titled = windows
        .iter()
        .filter(|window| window.title == child.title)
        .count();
    match (exact, titled) {
        (0, 0) => Ok(true),
        (1, 1) => Ok(false),
        _ => Err(format!(
            "retired X11 child identity is inconsistent: window={}, title={:?}, exact_matches={exact}, title_matches={titled}",
            child.window, child.title
        )),
    }
}

#[derive(Clone)]
pub(super) struct WindowRecord {
    pub(super) window: Window,
    pub(super) title: String,
    pub(super) bounds: PhysicalBounds,
}

struct X11Atoms {
    client_list_stacking: Atom,
    net_wm_name: Atom,
    net_wm_pid: Atom,
    utf8_string: Atom,
}

pub(super) struct X11Desktop {
    connection: RustConnection,
    root: Window,
    atoms: X11Atoms,
}

impl X11Desktop {
    pub(super) fn connect() -> Result<Self, String> {
        let (connection, screen) = x11rb::connect(None)
            .map_err(|error| format!("connect to the active X11 display: {error}"))?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| format!("X11 screen index {screen} is absent"))?
            .root;
        let atoms = X11Atoms {
            client_list_stacking: intern_atom(&connection, b"_NET_CLIENT_LIST_STACKING")?,
            net_wm_name: intern_atom(&connection, b"_NET_WM_NAME")?,
            net_wm_pid: intern_atom(&connection, b"_NET_WM_PID")?,
            utf8_string: intern_atom(&connection, b"UTF8_STRING")?,
        };
        let version = connection
            .xtest_get_version(2, 2)
            .map_err(|error| format!("query XTest extension version: {error}"))?
            .reply()
            .map_err(|error| format!("read XTest extension version: {error}"))?;
        if version.major_version < 2 {
            return Err(format!(
                "XTest 2.x is required, server reported {}.{}",
                version.major_version, version.minor_version
            ));
        }
        Ok(Self {
            connection,
            root,
            atoms,
        })
    }

    pub(super) fn process_windows(&self) -> Result<Vec<WindowRecord>, String> {
        let windows = self.required_u32_property(
            self.root,
            self.atoms.client_list_stacking,
            AtomEnum::WINDOW.into(),
            MAX_CLIENT_WINDOWS,
            "_NET_CLIENT_LIST_STACKING",
        )?;
        if windows.is_empty() {
            return Err(
                "_NET_CLIENT_LIST_STACKING is empty; run the smoke under an EWMH window manager"
                    .to_owned(),
            );
        }
        let current_pid = std::process::id();
        let mut records = Vec::new();
        for window in windows {
            let pid = match self.optional_u32_property(
                window,
                self.atoms.net_wm_pid,
                AtomEnum::CARDINAL.into(),
                1,
                "_NET_WM_PID",
            )? {
                None => None,
                Some(values) => match values.as_slice() {
                    [pid] => Some(*pid),
                    _ => {
                        return Err(format!(
                            "_NET_WM_PID on X11 window {window} must contain exactly one CARDINAL"
                        ));
                    }
                },
            };
            if pid != Some(current_pid) {
                continue;
            }
            records.push(WindowRecord {
                window,
                title: self.window_title(window)?,
                bounds: self.window_bounds(window)?,
            });
        }
        if records.is_empty() {
            return Err(format!(
                "the EWMH client list contains no windows for smoke process {current_pid}"
            ));
        }
        Ok(records)
    }

    pub(super) fn window_for_title(&self, title: &str) -> Result<WindowRecord, String> {
        let mut matches = self
            .process_windows()?
            .into_iter()
            .filter(|record| record.title == title)
            .collect::<Vec<_>>();
        match matches.len() {
            1 => Ok(matches.remove(0)),
            count => Err(format!(
                "expected one X11 client window titled {title:?} for this process, found {count}"
            )),
        }
    }

    pub(super) fn watch_destroy(&self, window: Window) -> Result<DestroyWatcher, String> {
        let (connection, screen) = x11rb::connect(None)
            .map_err(|error| format!("connect X11 destroy watcher: {error}"))?;
        let root = connection
            .setup()
            .roots
            .get(screen)
            .ok_or_else(|| format!("X11 watcher screen index {screen} is absent"))?
            .root;
        if root != self.root {
            return Err(format!(
                "X11 destroy watcher connected to root {root}, expected {}",
                self.root
            ));
        }
        connection
            .change_window_attributes(
                window,
                &ChangeWindowAttributesAux::new().event_mask(EventMask::STRUCTURE_NOTIFY),
            )
            .map_err(|error| {
                format!("subscribe to DestroyNotify for X11 window {window}: {error}")
            })?
            .check()
            .map_err(|error| {
                format!("X11 window {window} rejected DestroyNotify subscription: {error}")
            })?;
        connection
            .flush()
            .map_err(|error| format!("flush DestroyNotify subscription: {error}"))?;
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name(format!("dockspace-x11-destroy-{window}"))
            .spawn(move || {
                let result = loop {
                    match connection.wait_for_event() {
                        Ok(Event::DestroyNotify(event)) if event.window == window => {
                            break Ok(());
                        }
                        Ok(_) => {}
                        Err(error) => {
                            break Err(format!("wait for X11 DestroyNotify: {error}"));
                        }
                    }
                };
                let _ = sender.send(result);
            })
            .map_err(|error| {
                format!("spawn DestroyNotify watcher for X11 window {window}: {error}")
            })?;
        Ok(DestroyWatcher { receiver, window })
    }

    pub(super) fn move_pointer(&self, point: PhysicalPoint) -> Result<(), String> {
        let (x, y) = xtest_coordinates(point)?;
        self.connection
            .xtest_fake_input(MOTION_NOTIFY_EVENT, 0, CURRENT_TIME, self.root, x, y, 0)
            .map_err(|error| format!("inject XTest pointer motion to {point:?}: {error}"))?
            .check()
            .map_err(|error| format!("XTest pointer motion to {point:?} failed: {error}"))?;
        self.connection
            .flush()
            .map_err(|error| format!("flush XTest pointer motion: {error}"))?;
        self.assert_pointer(point, None)
    }

    pub(super) fn set_primary_button(&self, pressed: bool) -> Result<(), String> {
        let event = if pressed {
            BUTTON_PRESS_EVENT
        } else {
            BUTTON_RELEASE_EVENT
        };
        self.connection
            .xtest_fake_input(event, 1, CURRENT_TIME, self.root, 0, 0, 0)
            .map_err(|error| format!("inject XTest primary-button edge: {error}"))?
            .check()
            .map_err(|error| format!("XTest primary-button edge failed: {error}"))?;
        self.connection
            .flush()
            .map_err(|error| format!("flush XTest primary-button edge: {error}"))?;
        let pointer = self
            .connection
            .query_pointer(self.root)
            .map_err(|error| format!("query X11 pointer after button edge: {error}"))?
            .reply()
            .map_err(|error| format!("read X11 pointer after button edge: {error}"))?;
        let actual = pointer.mask.contains(KeyButMask::BUTTON1);
        if actual != pressed {
            return Err(format!(
                "XTest primary-button edge did not reach X11: expected pressed={pressed}, actual={actual}"
            ));
        }
        Ok(())
    }

    pub(super) fn assert_pointer(
        &self,
        expected: PhysicalPoint,
        pressed: Option<bool>,
    ) -> Result<(), String> {
        let pointer = self
            .connection
            .query_pointer(self.root)
            .map_err(|error| format!("query X11 pointer: {error}"))?
            .reply()
            .map_err(|error| format!("read X11 pointer: {error}"))?;
        let actual = PhysicalPoint {
            x: i32::from(pointer.root_x),
            y: i32::from(pointer.root_y),
        };
        if actual != expected {
            return Err(format!(
                "XTest pointer position mismatch: expected {expected:?}, actual {actual:?}"
            ));
        }
        if let Some(pressed) = pressed {
            let actual_pressed = pointer.mask.contains(KeyButMask::BUTTON1);
            if actual_pressed != pressed {
                return Err(format!(
                    "X11 primary-button state mismatch at {actual:?}: expected {pressed}, actual {actual_pressed}"
                ));
            }
        }
        Ok(())
    }

    fn required_u32_property(
        &self,
        window: Window,
        property: Atom,
        type_: Atom,
        max_items: usize,
        name: &str,
    ) -> Result<Vec<u32>, String> {
        self.optional_u32_property(window, property, type_, max_items, name)?
            .ok_or_else(|| format!("X11 property {name} is absent on window {window}"))
    }

    fn optional_u32_property(
        &self,
        window: Window,
        property: Atom,
        type_: Atom,
        max_items: usize,
        name: &str,
    ) -> Result<Option<Vec<u32>>, String> {
        let long_length = u32::try_from(max_items)
            .map_err(|_| format!("{name} item bound {max_items} exceeds u32"))?;
        let reply = self
            .connection
            .get_property(false, window, property, type_, 0, long_length)
            .map_err(|error| format!("query X11 property {name} on {window}: {error}"))?
            .reply()
            .map_err(|error| format!("read X11 property {name} on {window}: {error}"))?;
        parse_optional_u32_property(&reply, type_, max_items, name)
    }

    fn window_title(&self, window: Window) -> Result<String, String> {
        let long_length = u32::try_from(MAX_WINDOW_TITLE_BYTES.div_ceil(4))
            .expect("the title byte bound fits u32");
        let reply = self
            .connection
            .get_property(
                false,
                window,
                self.atoms.net_wm_name,
                self.atoms.utf8_string,
                0,
                long_length,
            )
            .map_err(|error| format!("query _NET_WM_NAME for {window}: {error}"))?
            .reply()
            .map_err(|error| format!("read _NET_WM_NAME for {window}: {error}"))?;
        let title = String::from_utf8(parse_utf8_property(
            reply,
            self.atoms.utf8_string,
            MAX_WINDOW_TITLE_BYTES,
            "_NET_WM_NAME",
        )?)
        .map_err(|error| format!("window {window} has a non-UTF-8 _NET_WM_NAME: {error}"))?;
        if title.contains('\0') {
            return Err(format!(
                "window {window} has an embedded NUL in _NET_WM_NAME"
            ));
        }
        Ok(title)
    }

    fn window_bounds(&self, window: Window) -> Result<PhysicalBounds, String> {
        let geometry = self
            .connection
            .get_geometry(window)
            .map_err(|error| format!("query geometry for X11 window {window}: {error}"))?
            .reply()
            .map_err(|error| format!("read geometry for X11 window {window}: {error}"))?;
        let origin = self
            .connection
            .translate_coordinates(window, self.root, 0, 0)
            .map_err(|error| format!("translate X11 window {window} to root: {error}"))?
            .reply()
            .map_err(|error| format!("read translated X11 window {window}: {error}"))?;
        let min_x = i32::from(origin.dst_x);
        let min_y = i32::from(origin.dst_y);
        let max_x = min_x
            .checked_add(i32::from(geometry.width))
            .ok_or_else(|| format!("X11 window {window} horizontal extent overflows"))?;
        let max_y = min_y
            .checked_add(i32::from(geometry.height))
            .ok_or_else(|| format!("X11 window {window} vertical extent overflows"))?;
        if max_x <= min_x || max_y <= min_y {
            return Err(format!("X11 window {window} has empty client geometry"));
        }
        Ok(PhysicalBounds {
            min_x,
            min_y,
            max_x,
            max_y,
        })
    }
}

pub(super) struct DestroyWatcher {
    receiver: mpsc::Receiver<Result<(), String>>,
    window: Window,
}

impl DestroyWatcher {
    pub(super) fn wait(self, deadline: Instant) -> Result<(), String> {
        let now = Instant::now();
        if now >= deadline {
            return Err(format!(
                "X11 window {} did not emit DestroyNotify before the deadline",
                self.window
            ));
        }
        match self
            .receiver
            .recv_timeout(deadline.saturating_duration_since(now))
        {
            Ok(Ok(())) => {
                eprintln!(
                    "physical-smoke: exact X11 DestroyNotify window={}",
                    self.window
                );
                Ok(())
            }
            Ok(Err(error)) => Err(error),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(format!(
                "X11 window {} did not emit DestroyNotify before the deadline",
                self.window
            )),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(format!(
                "X11 DestroyNotify watcher for window {} disconnected",
                self.window
            )),
        }
    }
}

fn parse_optional_u32_property(
    reply: &GetPropertyReply,
    expected_type: Atom,
    max_items: usize,
    name: &str,
) -> Result<Option<Vec<u32>>, String> {
    if reply.type_ == u32::from(AtomEnum::NONE) {
        if reply.format == 0
            && reply.length == 0
            && reply.bytes_after == 0
            && reply.value_len == 0
            && reply.value.is_empty()
        {
            return Ok(None);
        }
        return Err(format!("{name} has a malformed absent-property reply"));
    }
    let value_len = usize::try_from(reply.value_len)
        .map_err(|_| format!("{name} value length exceeds usize"))?;
    let expected_bytes = value_len
        .checked_mul(4)
        .ok_or_else(|| format!("{name} byte length overflows"))?;
    let expected_length =
        u32::try_from(value_len).map_err(|_| format!("{name} reply length exceeds u32"))?;
    if reply.type_ != expected_type
        || reply.format != 32
        || reply.bytes_after != 0
        || value_len > max_items
        || reply.value.len() != expected_bytes
        || reply.length != expected_length
    {
        return Err(format!(
            "{name} is not a complete bounded 32-bit property: type={}, expected_type={expected_type}, format={}, bytes_after={}, items={value_len}, max_items={max_items}, bytes={}, reply_length={}",
            reply.type_,
            reply.format,
            reply.bytes_after,
            reply.value.len(),
            reply.length
        ));
    }
    let values = reply
        .value32()
        .ok_or_else(|| format!("{name} did not expose 32-bit values"))?
        .collect::<Vec<_>>();
    if values.len() != value_len {
        return Err(format!(
            "{name} decoded {} values but declared {value_len}",
            values.len()
        ));
    }
    Ok(Some(values))
}

fn parse_utf8_property(
    reply: GetPropertyReply,
    expected_type: Atom,
    max_bytes: usize,
    name: &str,
) -> Result<Vec<u8>, String> {
    let value_len = usize::try_from(reply.value_len)
        .map_err(|_| format!("{name} value length exceeds usize"))?;
    let expected_length = u32::try_from(value_len.div_ceil(4))
        .map_err(|_| format!("{name} reply length exceeds u32"))?;
    if reply.type_ != expected_type
        || reply.format != 8
        || reply.bytes_after != 0
        || value_len > max_bytes
        || reply.value.len() != value_len
        || reply.length != expected_length
    {
        return Err(format!(
            "{name} is not a complete bounded UTF-8 property: type={}, expected_type={expected_type}, format={}, bytes_after={}, bytes={value_len}, max_bytes={max_bytes}, payload_bytes={}, reply_length={}",
            reply.type_,
            reply.format,
            reply.bytes_after,
            reply.value.len(),
            reply.length
        ));
    }
    Ok(reply.value)
}

fn intern_atom(connection: &RustConnection, name: &[u8]) -> Result<Atom, String> {
    connection
        .intern_atom(false, name)
        .map_err(|error| {
            format!(
                "intern X11 atom {:?}: {error}",
                String::from_utf8_lossy(name)
            )
        })?
        .reply()
        .map(|reply| reply.atom)
        .map_err(|error| {
            format!(
                "read interned X11 atom {:?}: {error}",
                String::from_utf8_lossy(name)
            )
        })
}

fn xtest_coordinates(point: PhysicalPoint) -> Result<(i16, i16), String> {
    let x = i16::try_from(point.x)
        .map_err(|_| format!("XTest x coordinate {} exceeds i16", point.x))?;
    let y = i16::try_from(point.y)
        .map_err(|_| format!("XTest y coordinate {} exceeds i16", point.y))?;
    Ok((x, y))
}

#[cfg(test)]
mod property_tests {
    use super::*;

    fn reply(
        format: u8,
        type_: Atom,
        bytes_after: u32,
        value_len: u32,
        value: Vec<u8>,
    ) -> GetPropertyReply {
        GetPropertyReply {
            format,
            length: u32::try_from(value.len().div_ceil(4)).expect("test value fits"),
            type_,
            bytes_after,
            value_len,
            value,
            ..Default::default()
        }
    }

    fn u32_bytes(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect()
    }

    #[test]
    fn bounded_u32_property_requires_exact_type_format_and_complete_length() {
        let expected_type = AtomEnum::CARDINAL.into();
        let exact = reply(32, expected_type, 0, 2, u32_bytes(&[7, 11]));
        assert_eq!(
            parse_optional_u32_property(&exact, expected_type, 2, "TEST"),
            Ok(Some(vec![7, 11]))
        );

        let wrong_type = reply(32, AtomEnum::WINDOW.into(), 0, 1, u32_bytes(&[7]));
        assert!(parse_optional_u32_property(&wrong_type, expected_type, 1, "TEST").is_err());
        let wrong_format = reply(8, expected_type, 0, 4, vec![7, 0, 0, 0]);
        assert!(parse_optional_u32_property(&wrong_format, expected_type, 1, "TEST").is_err());
        let truncated = reply(32, expected_type, 4, 1, u32_bytes(&[7]));
        assert!(parse_optional_u32_property(&truncated, expected_type, 1, "TEST").is_err());
        let mismatched_length = reply(32, expected_type, 0, 2, u32_bytes(&[7]));
        assert!(parse_optional_u32_property(&mismatched_length, expected_type, 2, "TEST").is_err());
        let mut wrong_reply_length = reply(32, expected_type, 0, 2, u32_bytes(&[7, 11]));
        wrong_reply_length.length = 3;
        assert!(
            parse_optional_u32_property(&wrong_reply_length, expected_type, 2, "TEST").is_err()
        );
        let oversized = reply(32, expected_type, 0, 2, u32_bytes(&[7, 11]));
        assert!(parse_optional_u32_property(&oversized, expected_type, 1, "TEST").is_err());
    }

    #[test]
    fn optional_u32_property_accepts_only_an_exact_absent_reply() {
        let expected_type = AtomEnum::CARDINAL.into();
        let absent = GetPropertyReply::default();
        assert_eq!(
            parse_optional_u32_property(&absent, expected_type, 1, "TEST"),
            Ok(None)
        );

        let malformed_absent = reply(32, AtomEnum::NONE.into(), 0, 0, Vec::new());
        assert!(parse_optional_u32_property(&malformed_absent, expected_type, 1, "TEST").is_err());
    }

    #[test]
    fn bounded_utf8_property_rejects_truncation_and_oversize() {
        let expected_type = 42;
        let exact = reply(8, expected_type, 0, 5, b"Child".to_vec());
        assert_eq!(
            parse_utf8_property(exact, expected_type, 5, "_NET_WM_NAME"),
            Ok(b"Child".to_vec())
        );

        let truncated = reply(8, expected_type, 1, 5, b"Child".to_vec());
        assert!(parse_utf8_property(truncated, expected_type, 5, "_NET_WM_NAME").is_err());
        let oversized = reply(8, expected_type, 0, 6, b"Child!".to_vec());
        assert!(parse_utf8_property(oversized, expected_type, 5, "_NET_WM_NAME").is_err());
        let wrong_type = reply(8, expected_type + 1, 0, 5, b"Child".to_vec());
        assert!(parse_utf8_property(wrong_type, expected_type, 5, "_NET_WM_NAME").is_err());
        let mut wrong_reply_length = reply(8, expected_type, 0, 5, b"Child".to_vec());
        wrong_reply_length.length = 3;
        assert!(parse_utf8_property(wrong_reply_length, expected_type, 5, "_NET_WM_NAME").is_err());
    }
}

#[cfg(test)]
mod retirement_tests {
    use super::*;

    fn window(window: Window, title: &str) -> WindowRecord {
        WindowRecord {
            window,
            title: title.to_owned(),
            bounds: PhysicalBounds {
                min_x: 0,
                min_y: 0,
                max_x: 100,
                max_y: 100,
            },
        }
    }

    #[test]
    fn x11_retirement_requires_the_unique_child_window_to_disappear() {
        let child = window(2, "Child");
        assert_eq!(
            x11_window_is_retired(&[window(1, "Root")], &child),
            Ok(true)
        );
        assert_eq!(
            x11_window_is_retired(&[window(1, "Root"), child.clone()], &child),
            Ok(false)
        );
        assert!(x11_window_is_retired(&[window(3, "Child")], &child).is_err());
    }
}
