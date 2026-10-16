//! Keystrokes into the Concat window. The API cannot touch the project on
//! screen, so editor control is the window's own shortcuts.

use std::time::Duration;

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, TCFType};
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use core_graphics::window::{
    copy_window_info, kCGNullWindowID, kCGWindowLayer, kCGWindowListExcludeDesktopElements,
    kCGWindowListOptionOnScreenOnly, kCGWindowOwnerName,
};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

pub const BUNDLE_ID: &str = "app.concat.editor";
const OWNER_NAME: &str = "Concat";

/// Serializes keystrokes so a fast dial spin arrives in order.
static SEND: Mutex<()> = Mutex::const_new(());

/// macOS virtual key codes (HIToolbox/Events.h).
mod vk {
    pub const A: u16 = 0x00;
    pub const S: u16 = 0x01;
    pub const D: u16 = 0x02;
    pub const H: u16 = 0x04;
    pub const Z: u16 = 0x06;
    pub const C: u16 = 0x08;
    pub const V: u16 = 0x09;
    pub const B: u16 = 0x0B;
    pub const E: u16 = 0x0E;
    pub const EQUAL: u16 = 0x18;
    pub const MINUS: u16 = 0x1B;
    pub const I: u16 = 0x22;
    pub const N: u16 = 0x2D;
    pub const M: u16 = 0x2E;
    pub const SPACE: u16 = 0x31;
    pub const DELETE: u16 = 0x33;
    pub const HOME: u16 = 0x73;
    pub const END: u16 = 0x77;
    pub const LEFT: u16 = 0x7B;
    pub const RIGHT: u16 = 0x7C;
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Command {
    #[default]
    Play,
    StepBack,
    StepForward,
    StepBack10,
    StepForward10,
    Start,
    End,
    Split,
    Razor,
    Delete,
    RippleDelete,
    Undo,
    Redo,
    Save,
    Copy,
    Paste,
    Duplicate,
    SelectAll,
    Snap,
    Mute,
    Pan,
    ZoomIn,
    ZoomOut,
    Export,
    Import,
    None,
}

/// Mirrors the bindings in concat/ui/app.slint. Slint's `Control` is
/// Command on macOS.
impl Command {
    fn chord(self) -> Option<(CGKeyCode, CGEventFlags)> {
        let none = CGEventFlags::CGEventFlagNull;
        let cmd = CGEventFlags::CGEventFlagCommand;
        let shift = CGEventFlags::CGEventFlagShift;
        Some(match self {
            Command::Play => (vk::SPACE, none),
            Command::StepBack => (vk::LEFT, none),
            Command::StepForward => (vk::RIGHT, none),
            Command::StepBack10 => (vk::LEFT, shift),
            Command::StepForward10 => (vk::RIGHT, shift),
            Command::Start => (vk::HOME, none),
            Command::End => (vk::END, none),
            Command::Split => (vk::B, cmd),
            Command::Razor => (vk::B, none),
            Command::Delete => (vk::DELETE, none),
            Command::RippleDelete => (vk::DELETE, shift),
            Command::Undo => (vk::Z, cmd),
            Command::Redo => (vk::Z, cmd | shift),
            Command::Save => (vk::S, cmd),
            Command::Copy => (vk::C, cmd),
            Command::Paste => (vk::V, cmd),
            Command::Duplicate => (vk::D, cmd),
            Command::SelectAll => (vk::A, cmd),
            Command::Snap => (vk::N, none),
            Command::Mute => (vk::M, none),
            Command::Pan => (vk::H, none),
            Command::ZoomIn => (vk::EQUAL, cmd),
            Command::ZoomOut => (vk::MINUS, cmd),
            Command::Export => (vk::E, cmd),
            Command::Import => (vk::I, cmd),
            Command::None => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Command::Play => "Play",
            Command::StepBack => "-1f",
            Command::StepForward => "+1f",
            Command::StepBack10 => "-10f",
            Command::StepForward10 => "+10f",
            Command::Start => "Start",
            Command::End => "End",
            Command::Split => "Split",
            Command::Razor => "Razor",
            Command::Delete => "Delete",
            Command::RippleDelete => "Ripple\nDelete",
            Command::Undo => "Undo",
            Command::Redo => "Redo",
            Command::Save => "Save",
            Command::Copy => "Copy",
            Command::Paste => "Paste",
            Command::Duplicate => "Dupe",
            Command::SelectAll => "Select\nAll",
            Command::Snap => "Snap",
            Command::Mute => "Mute",
            Command::Pan => "Pan",
            Command::ZoomIn => "Zoom +",
            Command::ZoomOut => "Zoom -",
            Command::Export => "Export",
            Command::Import => "Import",
            Command::None => "",
        }
    }
}

#[derive(Debug)]
pub enum KeyError {
    NotRunning,
    EventSource,
}

/// Sends `command` `times` times, bringing Concat forward first.
pub async fn send(command: Command, times: u32) -> Result<(), KeyError> {
    let Some((code, flags)) = command.chord() else {
        return Ok(());
    };
    let _guard = SEND.lock().await;
    ensure_frontmost().await?;
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| KeyError::EventSource)?;
    for _ in 0..times {
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(source.clone(), code, down)
                .map_err(|_| KeyError::EventSource)?;
            event.set_flags(flags);
            event.post(CGEventTapLocation::HID);
        }
    }
    Ok(())
}

async fn ensure_frontmost() -> Result<(), KeyError> {
    if frontmost_owner().as_deref() == Some(OWNER_NAME) {
        return Ok(());
    }
    if !is_running() {
        return Err(KeyError::NotRunning);
    }
    activate().await;
    for _ in 0..20 {
        tokio::time::sleep(Duration::from_millis(25)).await;
        if frontmost_owner().as_deref() == Some(OWNER_NAME) {
            // The window needs a beat after activation before it takes keys.
            tokio::time::sleep(Duration::from_millis(50)).await;
            return Ok(());
        }
    }
    Err(KeyError::NotRunning)
}

/// Brings Concat forward, launching it when it is not running.
pub async fn activate() {
    let _ = tokio::process::Command::new("/usr/bin/open")
        .args(["-b", BUNDLE_ID])
        .status()
        .await;
}

pub fn is_running() -> bool {
    std::process::Command::new("/usr/bin/pgrep")
        .args(["-xq", OWNER_NAME])
        .status()
        .is_ok_and(|status| status.success())
}

/// Owner of the frontmost normal window. NSWorkspace.frontmostApplication
/// goes stale in a process with no AppKit run loop; the window server's
/// list does not.
fn frontmost_owner() -> Option<String> {
    let windows: CFArray<CFDictionary<CFString, CFType>> = unsafe {
        let raw = copy_window_info(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        )?;
        CFArray::wrap_under_get_rule(raw.as_concrete_TypeRef())
    };
    let layer_key = unsafe { CFString::wrap_under_get_rule(kCGWindowLayer) };
    let owner_key = unsafe { CFString::wrap_under_get_rule(kCGWindowOwnerName) };
    windows.iter().find_map(|window| {
        let layer = window.find(&layer_key)?.downcast::<CFNumber>()?.to_i32()?;
        if layer != 0 {
            return None;
        }
        Some(window.find(&owner_key)?.downcast::<CFString>()?.to_string())
    })
}
