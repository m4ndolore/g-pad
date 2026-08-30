//! Raw multitouch gestures for takeover mode.
//! Drawer-aware touch routing plus the deliberate five-finger exit.

use std::io;
use std::os::fd::RawFd;

use crate::{evdev, fb};

const EV_SYN: u16 = 0;
const SYN_REPORT: u16 = 0;
const EV_ABS: u16 = 3;
const ABS_MT_SLOT: u16 = 47;
const ABS_MT_POSITION_X: u16 = 53;
const ABS_MT_POSITION_Y: u16 = 54;
const ABS_MT_TRACKING_ID: u16 = 57;
const EVIOCGRAB: libc::c_ulong = 0x40044590;
const MAX_SLOTS: usize = 16;
const TAP_SLOP: i32 = 45;
const EDGE_PX: i32 = 72;
const SWIPE_PX: i32 = 120;
// Paper Pro reports a larger raw range than the panel. rM2 pt_mt is already
// panel-sized (1404×1872) with Y growing toward the physical top.
#[cfg(not(feature = "rm2"))]
const TOUCH_MAX_X: i32 = 2064;
#[cfg(not(feature = "rm2"))]
const TOUCH_MAX_Y: i32 = 2832;
#[cfg(feature = "rm2")]
const TOUCH_MAX_X: i32 = 1403;
#[cfg(feature = "rm2")]
const TOUCH_MAX_Y: i32 = 1871;
// Require a deliberate hold before five-finger exit.  A single frame can be
// produced by a writing-hand/palm contact on the reMarkable touch sensor.
const FIVE_FINGER_HOLD_FRAMES: usize = 20;
/// Drift the contact centroid is allowed per frame of a five-finger hold.
/// Real holds measured on an rM2 ran to ~14 units a frame; a hand deliberately
/// swiped across the panel runs far above this.
const DRIFT_PER_FRAME: i32 = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    Quit,
    Undo,
    Redo,
    /// Positive values move down through the document.
    Scroll(i32),
    /// Direction (+1 down, -1 up); caller chooses page size.
    Page(i32),
    /// A rightward swipe beginning in the reserved left-edge zone.
    OpenDrawer,
    /// A leftward horizontal swipe. Only closes an already-open overlay.
    CloseDrawer,
    /// A downward swipe beginning at the top edge reveals Guided controls.
    OpenControls,
    /// Screen-space one-finger tap for fixed UI hit regions.
    Tap(i32, i32),
}

#[derive(Clone, Copy, Default)]
struct Slot {
    active: bool,
    start_x: i32,
    start_y: i32,
    x: i32,
    y: i32,
}

pub struct TouchDevice {
    fd: RawFd,
    slots: [Slot; MAX_SLOTS],
    cur: usize,
    max_fingers: usize,
    frame_x: Option<i32>,
    frame_y: Option<i32>,
    total_motion: i32,
    five_finger_hold_frames: usize,
}

impl TouchDevice {
    pub fn open() -> io::Result<Self> {
        for i in 0..8 {
            let name_path = format!("/sys/class/input/event{i}/device/name");
            if let Ok(name) = std::fs::read_to_string(&name_path) {
                let name = name.to_lowercase();
                // "touch" on the Paper Pro; "pt_mt"/"cyttsp5_mt" on rM2/rM1.
                if name.contains("touch") || name.contains("pt_mt") || name.contains("cyttsp5") {
                    let path = std::ffi::CString::new(format!("/dev/input/event{i}")).unwrap();
                    let fd =
                        unsafe { libc::open(path.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
                    if fd < 0 {
                        return Err(io::Error::last_os_error());
                    }
                    unsafe { libc::ioctl(fd, EVIOCGRAB as _, 1i32) };
                    return Ok(Self {
                        fd,
                        slots: [Slot::default(); MAX_SLOTS],
                        cur: 0,
                        max_fingers: 0,
                        frame_x: None,
                        frame_y: None,
                        total_motion: 0,
                        five_finger_hold_frames: 0,
                    });
                }
            }
        }
        Err(io::Error::new(io::ErrorKind::NotFound, "no touch device"))
    }

    /// Drain and discard touch input, then cancel every partial gesture. Used
    /// for palm rejection while the marker is in digitizer proximity.
    pub fn suppress(&mut self) {
        let _ = self.drain();
        self.slots = [Slot::default(); MAX_SLOTS];
        self.max_fingers = 0;
        self.frame_x = None;
        self.frame_y = None;
        self.total_motion = 0;
        self.five_finger_hold_frames = 0;
    }

    /// Compatibility helper for takeover apps that only use five-finger exit.
    pub fn drain_check_quit(&mut self) -> bool {
        self.drain().contains(&Gesture::Quit)
    }

    pub fn drain(&mut self) -> Vec<Gesture> {
        let mut out = Vec::new();
        let mut buf = [0u8; evdev::EV_SIZE * 64];
        loop {
            let n =
                unsafe { libc::read(self.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
            if n <= 0 {
                break;
            }
            for chunk in buf[..n as usize].chunks_exact(evdev::EV_SIZE) {
                let (etype, code, value) = evdev::decode(chunk);
                if etype == EV_ABS && code == ABS_MT_SLOT {
                    self.cur = (value.max(0) as usize).min(MAX_SLOTS - 1);
                } else if etype == EV_ABS && code == ABS_MT_POSITION_Y {
                    self.slots[self.cur].y = value;
                    if self.slots[self.cur].active && self.slots[self.cur].start_y == i32::MIN {
                        self.slots[self.cur].start_y = value;
                    }
                } else if etype == EV_ABS && code == ABS_MT_POSITION_X {
                    self.slots[self.cur].x = value;
                    if self.slots[self.cur].active && self.slots[self.cur].start_x == i32::MIN {
                        self.slots[self.cur].start_x = value;
                    }
                } else if etype == EV_ABS && code == ABS_MT_TRACKING_ID {
                    if value != -1 {
                        self.slots[self.cur] = Slot {
                            active: true,
                            start_x: i32::MIN,
                            start_y: i32::MIN,
                            x: self.slots[self.cur].x,
                            y: self.slots[self.cur].y,
                        };
                    } else {
                        self.slots[self.cur].active = false;
                    }
                } else if etype == EV_SYN && code == SYN_REPORT {
                    self.finish_frame(&mut out);
                }
            }
        }
        out
    }

    fn finish_frame(&mut self, out: &mut Vec<Gesture>) {
        let active: Vec<Slot> = self.slots.iter().copied().filter(|s| s.active).collect();
        let count = active.len();
        self.max_fingers = self.max_fingers.max(count);
        if count >= 5 {
            self.five_finger_hold_frames = self.five_finger_hold_frames.saturating_add(1);
        }

        let average_x = (count > 0).then(|| active.iter().map(|s| s.x).sum::<i32>() / count as i32);
        let average_y = (count > 0).then(|| active.iter().map(|s| s.y).sum::<i32>() / count as i32);
        if let (Some(previous), Some(current)) = (self.frame_x, average_x) {
            self.total_motion += (previous - current).abs();
        }
        if let (Some(previous), Some(current)) = (self.frame_y, average_y) {
            let raw_delta = previous - current;
            self.total_motion += raw_delta.abs();
            if count == 2 {
                let pixels = raw_delta * fb::SCREEN_H as i32 / TOUCH_MAX_Y;
                if pixels != 0 {
                    out.push(Gesture::Scroll(pixels));
                }
            }
        }
        self.frame_y = average_y;
        self.frame_x = average_x;

        if count == 0 && self.max_fingers > 0 {
            // A multi-finger release that is not recognised is invisible from
            // the outside: the pad simply does nothing. Say what was measured
            // so a missed gesture can be told from an unsupported one.
            if self.max_fingers >= 4 {
                eprintln!(
                    "g-pad: touch release fingers={} hold_frames={} motion={} (quit needs >={} frames, motion <{})",
                    self.max_fingers, self.five_finger_hold_frames, self.total_motion,
                    FIVE_FINGER_HOLD_FRAMES, TAP_SLOP,
                );
            }
            if five_finger_release_is_quit(
                self.max_fingers,
                self.five_finger_hold_frames,
                self.total_motion,
            ) {
                out.push(Gesture::Quit);
            } else if self.total_motion < TAP_SLOP {
                match self.max_fingers {
                    2 => out.push(Gesture::Undo),
                    3 => out.push(Gesture::Redo),
                    1 => {
                        if let Some(slot) = self.slots.iter().find(|s| s.start_y != i32::MIN && s.start_x != i32::MIN) {
                            let (x, y) = map_touch(slot.x, slot.y);
                            out.push(Gesture::Tap(x, y));
                        }
                    }
                    _ => {}
                }
            } else if self.max_fingers == 1 {
                // Released slots retain their coordinates.
                if let Some(slot) = self
                    .slots
                    .iter()
                    .filter(|slot| slot.start_y != i32::MIN && slot.start_x != i32::MIN)
                    .max_by_key(|slot| (slot.start_y - slot.y).abs() + (slot.start_x - slot.x).abs())
                {
                    let (x0, y0) = map_touch(slot.start_x, slot.start_y);
                    let (x1, y1) = map_touch(slot.x, slot.y);
                    out.push(classify_swipe(x0, y0, x1, y1));
                }
            }
            self.max_fingers = 0;
            self.frame_x = None;
            self.frame_y = None;
            self.total_motion = 0;
            self.five_finger_hold_frames = 0;
        }
    }
}

#[cfg(not(feature = "rm2"))]
fn map_touch(raw_x: i32, raw_y: i32) -> (i32, i32) {
    (
        raw_x.max(0) * fb::SCREEN_W as i32 / TOUCH_MAX_X,
        raw_y.max(0) * fb::SCREEN_H as i32 / TOUCH_MAX_Y,
    )
}

#[cfg(feature = "rm2")]
fn map_touch(raw_x: i32, raw_y: i32) -> (i32, i32) {
    // pt_mt origin is the physical bottom-left. Framebuffer y=0 is the top.
    let x = raw_x.clamp(0, TOUCH_MAX_X) * (fb::SCREEN_W as i32 - 1) / TOUCH_MAX_X;
    let y = (TOUCH_MAX_Y - raw_y.clamp(0, TOUCH_MAX_Y)) * (fb::SCREEN_H as i32 - 1) / TOUCH_MAX_Y;
    (x, y)
}

fn classify_swipe(x0: i32, y0: i32, x1: i32, y1: i32) -> Gesture {
    let (dx, dy) = (x1 - x0, y1 - y0);
    if x0 <= EDGE_PX && dx >= SWIPE_PX && dx.abs() > dy.abs() {
        Gesture::OpenDrawer
    } else if dx <= -SWIPE_PX && dx.abs() > dy.abs() {
        Gesture::CloseDrawer
    } else if y0 <= EDGE_PX && dy >= SWIPE_PX && dy.abs() > dx.abs() {
        Gesture::OpenControls
    } else if y0 >= fb::SCREEN_H as i32 - EDGE_PX && dy <= -SWIPE_PX && dy.abs() > dx.abs() {
        Gesture::OpenControls
    } else {
        Gesture::Page((-dy).signum())
    }
}

/// Classify screen-space points supplied by window-system touch fallback.
pub fn gesture_from_points(start: (i32, i32), end: (i32, i32)) -> Gesture {
    if (end.0 - start.0).abs() + (end.1 - start.1).abs() < TAP_SLOP {
        Gesture::Tap(end.0, end.1)
    } else {
        classify_swipe(start.0, start.1, end.0, end.1)
    }
}

/// How far the contact centroid may drift per frame and still count as held.
///
/// Five fingers resting on glass are never perfectly still: the centroid moves
/// as pressure shifts between them. Measured holds drifted ~3-15 units a frame.
fn five_finger_release_is_quit(max_fingers: usize, hold_frames: usize, motion: i32) -> bool {
    if max_fingers < 5 || hold_frames < FIVE_FINGER_HOLD_FRAMES {
        return false;
    }
    // A budget proportional to the hold, not a fixed total. Charging drift
    // against a constant made a longer, more deliberate hold *harder* to
    // perform — the opposite of what the gesture asks for — and rejected
    // every real attempt.
    motion <= TAP_SLOP + DRIFT_PER_FRAME * hold_frames as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_finger_quit_requires_a_stationary_hold() {
        assert!(!five_finger_release_is_quit(5, FIVE_FINGER_HOLD_FRAMES - 1, 0));
        assert!(five_finger_release_is_quit(5, FIVE_FINGER_HOLD_FRAMES, 0));
        // Fewer than five fingers is never a quit, however still it is held.
        assert!(!five_finger_release_is_quit(4, FIVE_FINGER_HOLD_FRAMES * 4, 0));
    }

    // Measured on an rM2 from five real five-finger holds. Every one of them
    // was rejected by a fixed motion budget: the hold was 2.4-5.5x longer than
    // required, and drift accumulates every frame, so holding deliberately
    // made the gesture strictly harder to perform. A resting hand is never
    // perfectly still; the budget has to be a rate, not a total.
    #[test]
    fn a_deliberate_hand_is_not_disqualified_by_its_own_drift() {
        for (hold_frames, motion) in [(48, 140), (50, 133), (75, 1081), (94, 589), (111, 300)] {
            assert!(
                five_finger_release_is_quit(5, hold_frames, motion),
                "a real five-finger hold ({hold_frames} frames, motion {motion}) must quit",
            );
        }
    }

    #[test]
    fn a_five_finger_swipe_is_still_not_a_quit() {
        // Deliberately dragging the whole hand across the panel far outruns
        // the per-frame budget and must stay a non-event.
        assert!(!five_finger_release_is_quit(5, 40, DRIFT_PER_FRAME * 40 * 4));
    }

    #[test]
    fn edge_swipe_is_reserved_for_drawer_not_page_navigation() {
        assert_eq!(classify_swipe(20, 500, 240, 510), Gesture::OpenDrawer);
        assert_eq!(classify_swipe(200, 500, 210, 250), Gesture::Page(1));
        assert_eq!(classify_swipe(300, 500, 100, 510), Gesture::CloseDrawer);
        assert_eq!(classify_swipe(200, 10, 210, 200), Gesture::OpenControls);
        assert_eq!(
            classify_swipe(200, fb::SCREEN_H as i32 - 10, 210, fb::SCREEN_H as i32 - 200),
            Gesture::OpenControls
        );
    }

    #[cfg(feature = "rm2")]
    #[test]
    fn rm2_touch_maps_panel_extents_with_y_inverted() {
        assert_eq!(map_touch(0, 0), (0, fb::SCREEN_H as i32 - 1));
        assert_eq!(map_touch(TOUCH_MAX_X, TOUCH_MAX_Y), (fb::SCREEN_W as i32 - 1, 0));
        let (x, y) = map_touch(TOUCH_MAX_X / 2, TOUCH_MAX_Y / 2);
        assert!(x > 600 && x < 800, "mid x was {x}");
        assert!(y > 800 && y < 1100, "mid y was {y}");
    }
}

impl Drop for TouchDevice {
    fn drop(&mut self) {
        unsafe {
            libc::ioctl(self.fd, EVIOCGRAB as _, 0i32);
            libc::close(self.fd);
        }
    }
}
