//! Draw with Trackpad: raw touchpad contacts behind a safe, platform-specific API (AppKit
//! touches and Force Touch pressure on macOS, evdev on Linux, precision-touchpad HID on Windows).
//! No cursor-motion fallback: without a backend the mode is unavailable.
use gridcraft_ui_egui::{TrackpadCapture, TrackpadFrame, TrackpadPress};

/// Contact frames from a backend that doesn't measure pressure.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn touched(frames: Vec<Vec<[f32; 2]>>) -> Vec<TrackpadFrame> {
    frames.into_iter().map(|fingers| TrackpadFrame { fingers, press: TrackpadPress::Unmeasured }).collect()
}

/// The capture service. Devices are opened when the mode is first turned on (never at startup,
/// which would block launch and, on Linux, open every input device) and released when it's
/// turned off. The slow part of the check (macOS asks the I/O registry) runs in the background
/// from launch, so turning the mode on doesn't stall the canvas's opening.
pub fn service() -> (Option<TrackpadCapture>, Option<String>) {
    if !platform::SUPPORTED {
        return (None, Some("Trackpad capture unavailable".into()));
    }
    let mut probe = std::thread::Builder::new().name("trackpad-probe".into()).spawn(platform::probe).ok();
    let mut present = false;
    let mut backend: Option<platform::Backend> = None;
    let capture = move |enabled: bool| {
        if !enabled {
            let released = backend.as_mut().map_or(Ok(Vec::new()), |b| b.poll(false));
            backend = None;
            return released;
        }
        let device = match backend.as_mut() {
            Some(device) => device,
            None => {
                // A trackpad found once stays found; a missing one is looked for again (it may
                // have been connected since).
                if !present {
                    match probe.take() {
                        Some(handle) => handle.join().unwrap_or_else(|_| Err("Trackpad capture unavailable".into()))?,
                        None => platform::probe()?,
                    }
                    present = true;
                }
                backend.insert(platform::Backend::new()?)
            }
        };
        device.poll(true)
    };
    (Some(Box::new(capture)), None)
}

/// While a Draw tool is in use: every pointer move (macOS merges mouse drags by default, which
/// thins fast strokes) and Force Touch's generic pressure, so pressing hard mid-stroke doesn't
/// force-click (Look Up). Called every frame.
pub fn precise_pointer(on: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSApplication, NSEvent, NSPressureBehavior, NSPressureConfiguration};
        let Some(mtm) = objc2::MainThreadMarker::new() else { return };
        thread_local!(static CURRENT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) });
        if CURRENT.with(|c| c.replace(Some(on))) != Some(on) {
            NSEvent::setMouseCoalescingEnabled(!on);
        }
        // The key window can come and go (dialogs): checked each frame, set only when it differs.
        if let Some(view) = NSApplication::sharedApplication(mtm).keyWindow().and_then(|w| w.contentView()) {
            let generic = view.pressureConfiguration().is_some_and(|c| c.pressureBehavior() == NSPressureBehavior::PrimaryGeneric);
            if on && !generic {
                use objc2::AllocAnyThread;
                let config = NSPressureConfiguration::initWithPressureBehavior(NSPressureConfiguration::alloc(), NSPressureBehavior::PrimaryGeneric);
                view.setPressureConfiguration(Some(&config));
            } else if !on && generic {
                view.setPressureConfiguration(None);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = on;
}

#[cfg(target_os = "macos")]
mod platform {
    pub const SUPPORTED: bool = true;
    use gridcraft_ui_egui::{TrackpadFrame, TrackpadPress};
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol};
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{
        NSApplication, NSEvent, NSEventMask, NSEventType, NSPressureBehavior, NSPressureConfiguration, NSTouchPhase, NSTouchType, NSTouchTypeMask,
        NSView,
    };
    use objc2_foundation::{NSDate, NSString};

    /// The fingers on the pad, and the identity of the first (a new identity is a new finger).
    fn contact_frame(event: &NSEvent, view: Option<&NSView>) -> Option<(Vec<[f32; 2]>, Option<Retained<AnyObject>>)> {
        // AppKit asserts when touchesMatchingPhase:inView: receives a mouse/key event,
        // including a mouse-up with NSEventSubtypeTouch. Foreign exceptions abort Rust.
        // Only raw, generic touch events belong here; interpreted gestures aren't frames.
        if event.r#type() != NSEventType::Gesture {
            return None;
        }
        let contacts = event.touchesMatchingPhase_inView(NSTouchPhase::Touching, view);
        let mut first = None;
        let mut fingers = Vec::new();
        for touch in contacts.iter().take(16).filter(|touch| touch.r#type() == NSTouchType::Indirect && !touch.isResting()) {
            let q = touch.normalizedPosition();
            if q.x.is_finite() && q.y.is_finite() {
                first.get_or_insert_with(|| touch.identity());
                fingers.push([q.x.clamp(0.0, 1.0) as f32, (1.0 - q.y).clamp(0.0, 1.0) as f32]);
            }
        }
        Some((fingers, first))
    }

    /// Two touch identities are the same finger (`isEqual:`, as AppKit asks; else the same object).
    fn same_finger(a: &AnyObject, b: &AnyObject) -> bool {
        match (a.downcast_ref::<NSObject>(), b.downcast_ref::<NSObject>()) {
            (Some(a), Some(b)) => a.isEqual(Some(b)),
            _ => std::ptr::eq(a, b),
        }
    }
    /// A trackpad is attached (any thread: it only asks the I/O registry).
    pub fn probe() -> Result<(), String> {
        let result =
            std::process::Command::new("/usr/sbin/ioreg").args(["-r", "-c", "AppleMultitouchDevice", "-l"]).output().map_err(|e| e.to_string())?;
        let present =
            String::from_utf8_lossy(&result.stdout).lines().any(|line| line.contains("\"Product\"") && line.to_lowercase().contains("trackpad"));
        if !result.status.success() || !present {
            return Err("No trackpad detected".into());
        }
        Ok(())
    }
    /// The run-loop mode the touch events are drained in. Spinning the default mode inside
    /// winit's handler re-entered its observers and swallowed its redraw wake-ups: the UI froze
    /// for up to two seconds each time the mode was turned on.
    const RUN_LOOP_MODE: &str = "com.gridcraft.trackpad";

    /// The press a Force Touch pad reports: touched until it's clicked down (stage 0), then the
    /// stage-1 pressure. Pressure events and the left button's down, drag and up carry it;
    /// AppKit raises for `pressure` on any other type, and for `stage` on anything but pressure.
    fn pressure(event: &NSEvent) -> Option<TrackpadPress> {
        let clicked = |p: f32| TrackpadPress::Clicked(if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 });
        match event.r#type() {
            NSEventType::Pressure => Some(if event.stage() >= 1 { clicked(event.pressure()) } else { TrackpadPress::Touched }),
            NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged => Some(clicked(event.pressure())),
            NSEventType::LeftMouseUp => Some(TrackpadPress::Touched),
            _ => None,
        }
    }

    /// The view's settings before the mode changed them, put back when it ends.
    struct Saved {
        touch_types: NSTouchTypeMask,
        resting: bool,
        pressure: Option<Retained<NSPressureConfiguration>>,
    }

    pub struct Backend {
        active: bool,
        timestamp: f64,
        view: Option<Retained<NSView>>,
        previous: Option<Saved>,
        /// The pad's press now, and the fingers of the last contact frame (a press without a
        /// move repeats them with the new pressure).
        press: TrackpadPress,
        fingers: Vec<[f32; 2]>,
        /// The drawing finger's identity: another finger is a new stroke even when the lift between
        /// them was never seen.
        finger: Option<Retained<AnyObject>>,
    }
    impl Backend {
        pub fn new() -> Result<Self, String> {
            Ok(Self { active: false, timestamp: -1.0, view: None, previous: None, press: TrackpadPress::Touched, fingers: Vec::new(), finger: None })
        }
        fn release(&mut self) {
            if let Some(view) = self.view.take()
                && let Some(saved) = self.previous.take()
            {
                view.setAllowedTouchTypes(saved.touch_types);
                view.setWantsRestingTouches(saved.resting);
                view.setPressureConfiguration(saved.pressure.as_deref());
            }
            self.active = false;
            self.press = TrackpadPress::Touched;
            self.fingers.clear();
            self.finger = None;
        }
        pub fn poll(&mut self, enabled: bool) -> Result<Vec<TrackpadFrame>, String> {
            let mtm = MainThreadMarker::new().ok_or("Trackpad capture unavailable")?;
            let app = NSApplication::sharedApplication(mtm);
            if !enabled {
                self.release();
                return Ok(Vec::new());
            }
            if !self.active {
                let window = app.keyWindow().ok_or("Trackpad capture unavailable")?;
                let view = window.contentView().ok_or("Trackpad capture unavailable")?;
                self.previous = Some(Saved {
                    touch_types: view.allowedTouchTypes(),
                    resting: view.wantsRestingTouches(),
                    pressure: view.pressureConfiguration(),
                });
                view.setAllowedTouchTypes(NSTouchTypeMask::Indirect);
                view.setWantsRestingTouches(false);
                // Continuous stage-1 pressure from the click down, with no force click (and so
                // no Look Up or other system action) at the end of the travel: Apple's behaviour
                // for drawing.
                let generic = NSPressureConfiguration::initWithPressureBehavior(NSPressureConfiguration::alloc(), NSPressureBehavior::PrimaryGeneric);
                view.setPressureConfiguration(Some(&generic));
                self.view = Some(view);
                self.active = true;
            }
            let view = self.view.clone().ok_or("Trackpad capture unavailable")?;
            let mode = NSString::from_str(RUN_LOOP_MODE);
            let mask = NSEventMask::Gesture | NSEventMask::Pressure;
            let past = NSDate::distantPast();
            let mut frames = Vec::new();
            // Drain bounded bursts, keeping every contact frame so fast strokes keep their shape.
            // This spins the run loop once without waiting (`distantPast`), inside winit's
            // handler; only gesture events are dequeued. The private mode keeps that spin away
            // from winit's and Core Animation's observers (registered for the common modes).
            for _ in 0..128 {
                let Some(event) = app.nextEventMatchingMask_untilDate_inMode_dequeue(mask, Some(&past), &mode, true) else { break };
                frames.extend(self.frame(&event, &view));
                self.timestamp = event.timestamp();
            }
            if frames.is_empty()
                && let Some(event) = app.currentEvent()
                && event.timestamp() != self.timestamp
            {
                frames.extend(self.frame(&event, &view));
                self.timestamp = event.timestamp();
            }
            Ok(frames)
        }
        /// The contact frames an event makes: new fingers, or the same fingers pressed
        /// differently. A different finger first lifts the one before (its lift may have been
        /// missed), so two strokes are never joined.
        fn frame(&mut self, event: &NSEvent, view: &NSView) -> Vec<TrackpadFrame> {
            if let Some(press) = pressure(event) {
                self.press = press;
                return if self.fingers.is_empty() { Vec::new() } else { vec![TrackpadFrame { fingers: self.fingers.clone(), press }] };
            }
            let Some((fingers, first)) = contact_frame(event, Some(view)) else { return Vec::new() };
            let mut frames = Vec::new();
            if let (Some(before), Some(now)) = (&self.finger, &first)
                && !self.fingers.is_empty()
                && !same_finger(before, now)
            {
                frames.push(TrackpadFrame { fingers: Vec::new(), press: self.press });
            }
            self.finger = first;
            self.fingers.clone_from(&fingers);
            frames.push(TrackpadFrame { fingers, press: self.press });
            frames
        }
    }
    impl Drop for Backend {
        fn drop(&mut self) {
            self.release();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use objc2_app_kit::NSEventModifierFlags;
        use objc2_foundation::NSPoint;

        #[test]
        fn mouse_events_do_not_query_appkit_touches() {
            for kind in [NSEventType::LeftMouseDown, NSEventType::LeftMouseUp, NSEventType::MouseMoved, NSEventType::LeftMouseDragged] {
                let event = NSEvent::mouseEventWithType_location_modifierFlags_timestamp_windowNumber_context_eventNumber_clickCount_pressure(
                    kind,
                    NSPoint::new(20.0, 30.0),
                    NSEventModifierFlags::empty(),
                    1.0,
                    0,
                    None,
                    1,
                    1,
                    0.0,
                )
                .expect("synthetic native mouse event");
                // The previous implementation throws NSInternalInconsistencyException here.
                assert!(contact_frame(&event, None).is_none());
                // The left button carries the press; `stage` is only read from pressure events.
                let expected = match kind {
                    NSEventType::LeftMouseUp => Some(TrackpadPress::Touched),
                    NSEventType::MouseMoved => None,
                    _ => Some(TrackpadPress::Clicked(0.0)),
                };
                assert_eq!(pressure(&event), expected);
            }
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod platform {
    pub const SUPPORTED: bool = false;
    pub fn probe() -> Result<(), String> {
        Err("Trackpad capture unavailable".into())
    }
    pub struct Backend;
    impl Backend {
        pub fn new() -> Result<Self, String> {
            Err("Trackpad capture unavailable".into())
        }
        pub fn poll(&mut self, _: bool) -> Result<Vec<gridcraft_ui_egui::TrackpadFrame>, String> {
            Ok(Vec::new())
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    pub const SUPPORTED: bool = true;
    /// Devices are only opened when the mode starts.
    pub fn probe() -> Result<(), String> {
        Ok(())
    }
    use evdev::{AbsoluteAxisCode as Axis, Device, EventType, KeyCode, PropType};
    const SYN_REPORT: u16 = 0;
    const SYN_DROPPED: u16 = 3;
    pub struct Backend {
        device: Device,
        active: bool,
        slot: usize,
        /// Each slot's contact; NaN until its position arrives.
        slots: Vec<Option<[f32; 2]>>,
        bounds: [[i32; 2]; 2],
        /// The kernel dropped events: skip to the next report, then start from no contacts.
        dropped: bool,
    }
    impl Backend {
        pub fn new() -> Result<Self, String> {
            let mut readable = false;
            for (_, device) in evdev::enumerate() {
                readable = true;
                let touchpad = device.properties().contains(PropType::POINTER)
                    && !device.properties().contains(PropType::DIRECT)
                    && device.supported_keys().is_some_and(|keys| keys.contains(KeyCode::BTN_TOOL_FINGER));
                if !touchpad {
                    continue;
                }
                let axes = device.supported_absolute_axes();
                if !axes.is_some_and(|axes| {
                    [Axis::ABS_MT_SLOT, Axis::ABS_MT_TRACKING_ID, Axis::ABS_MT_POSITION_X, Axis::ABS_MT_POSITION_Y].iter().all(|a| axes.contains(*a))
                }) {
                    continue;
                }
                let mut bounds = [[0, 0]; 2];
                for (axis, info) in device.get_absinfo().map_err(|e| e.to_string())? {
                    if axis == Axis::ABS_MT_POSITION_X {
                        bounds[0] = [info.minimum(), info.maximum()];
                    }
                    if axis == Axis::ABS_MT_POSITION_Y {
                        bounds[1] = [info.minimum(), info.maximum()];
                    }
                }
                if bounds.iter().any(|range| range[0] >= range[1]) {
                    continue;
                }
                device.set_nonblocking(true).map_err(|e| e.to_string())?;
                return Ok(Self { device, active: false, slot: 0, slots: vec![None; 32], bounds, dropped: false });
            }
            // Input devices exist but none could be opened: reading them needs the `input` group.
            let exists = std::fs::read_dir("/dev/input").is_ok_and(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("event")));
            Err(if exists && !readable { "No permission to read the trackpad" } else { "No trackpad detected" }.into())
        }
        pub fn poll(&mut self, enabled: bool) -> Result<Vec<gridcraft_ui_egui::TrackpadFrame>, String> {
            if enabled != self.active {
                if enabled {
                    self.device.grab().map_err(|e| format!("Trackpad capture unavailable: {e}"))?;
                } else {
                    self.device.ungrab().map_err(|e| e.to_string())?;
                }
                self.active = enabled;
                self.slots.fill(None);
            }
            if !enabled {
                return Ok(Vec::new());
            }
            let events = match self.device.fetch_events().map(|events| events.take(2048).collect::<Vec<_>>()) {
                Ok(events) => events,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(Vec::new()),
                Err(e) => {
                    let _ = self.device.ungrab();
                    self.active = false;
                    return Err(e.to_string());
                }
            };
            let mut frames = Vec::new();
            for event in events {
                if event.event_type() == EventType::SYNCHRONIZATION {
                    match event.code() {
                        SYN_DROPPED => self.dropped = true,
                        SYN_REPORT if self.dropped => {
                            self.dropped = false;
                            self.slots.fill(None);
                            frames.push(Vec::new());
                        }
                        // A complete report: the contacts whose positions are known.
                        SYN_REPORT => frames.push(self.slots.iter().flatten().filter(|p| p.iter().all(|v| v.is_finite())).copied().collect()),
                        _ => {}
                    }
                    continue;
                }
                if self.dropped || event.event_type() != EventType::ABSOLUTE {
                    continue;
                }
                let code = Axis(event.code());
                if code == Axis::ABS_MT_SLOT {
                    self.slot = usize::try_from(event.value()).unwrap_or(usize::MAX);
                    continue;
                }
                let Some(slot) = self.slots.get_mut(self.slot) else { continue };
                if code == Axis::ABS_MT_TRACKING_ID {
                    *slot = (event.value() >= 0).then_some([f32::NAN; 2]);
                    continue;
                }
                let axis = if code == Axis::ABS_MT_POSITION_X {
                    0
                } else if code == Axis::ABS_MT_POSITION_Y {
                    1
                } else {
                    continue;
                };
                if let Some(point) = slot.as_mut()
                    && let Some(range) = self.bounds.get(axis)
                    && let Some(v) = point.get_mut(axis)
                {
                    *v = ((event.value() as f64 - range[0] as f64) / (range[1] as f64 - range[0] as f64)).clamp(0.0, 1.0) as f32;
                }
            }
            Ok(super::touched(frames))
        }
    }
    impl Drop for Backend {
        fn drop(&mut self) {
            if self.active {
                let _ = self.device.ungrab();
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod platform {
    pub const SUPPORTED: bool = true;
    /// Devices are only opened when the mode starts.
    pub fn probe() -> Result<(), String> {
        Ok(())
    }
    use super::super::trackpad_hid::{Assembler, Descriptor};
    pub struct Backend {
        device: hidapi::HidDevice,
        descriptor: Descriptor,
        assembler: Assembler,
        active: bool,
    }
    impl Backend {
        pub fn new() -> Result<Self, String> {
            let api = hidapi::HidApi::new().map_err(|e| e.to_string())?;
            let mut found = false;
            for info in api.device_list().filter(|d| d.usage_page() == 0x0d && d.usage() == 0x05) {
                found = true;
                let Ok(device) = info.open_device(&api) else { continue };
                let mut bytes = [0u8; 4096];
                let Ok(n) = device.get_report_descriptor(&mut bytes) else { continue };
                let Some(descriptor) = bytes.get(..n).and_then(Descriptor::parse) else { continue };
                return Ok(Self { device, descriptor, assembler: Assembler::default(), active: false });
            }
            Err(if found { "Trackpad capture unavailable" } else { "No trackpad detected" }.into())
        }
        pub fn poll(&mut self, enabled: bool) -> Result<Vec<gridcraft_ui_egui::TrackpadFrame>, String> {
            if !enabled {
                self.active = false;
                self.assembler = Assembler::default();
                return Ok(Vec::new());
            }
            let mut bytes = [0u8; 4096];
            let mut frames = Vec::new();
            for _ in 0..128 {
                let n = self.device.read_timeout(&mut bytes, 0).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                if self.active
                    && let Some(packet) = bytes.get(..n).and_then(|data| self.descriptor.packet(data))
                    && let Some(points) = self.assembler.push(packet)
                {
                    frames.push(points);
                }
            }
            self.active = true;
            Ok(super::touched(frames))
        }
    }
}
