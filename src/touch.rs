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
// A page flip is as deliberate as any other swipe. Without a floor here the
// catch-all below turned every one-finger contact whose jitter cleared
// TAP_SLOP into a page turn — a palm resting on the sheet flipped the
// notebook out from under the writer.
const PAGE_PX: i32 = SWIPE_PX;
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
    /// Finger count of the previous frame; motion only accumulates while the
    /// count is stable, so the averaged point jumping as fingers land or lift
    /// is not mistaken for movement.
    frame_fingers: usize,
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
                        frame_fingers: 0,
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
        self.frame_fingers = 0;
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
        // A finger landing or lifting yanks the averaged point sideways; that
        // jump is a count change, not motion. Only frames with a stable count
        // accumulate motion or scroll.
        if count == self.frame_fingers {
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
        }
        self.frame_fingers = count;
        self.frame_y = average_y;
        self.frame_x = average_x;

        if count == 0 && self.max_fingers > 0 {
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
            } else if let Some(swipe) = self.lone_swipe() {
                out.push(swipe);
            }
            self.max_fingers = 0;
            self.frame_x = None;
            self.frame_y = None;
            self.total_motion = 0;
            self.five_finger_hold_frames = 0;
            // A released slot keeps its coordinates, which is what the release
            // frame above reads. Past that point they are stale, and a stale
            // start would be measured as travel in somebody else's gesture.
            self.slots = [Slot::default(); MAX_SLOTS];
        }
    }

    /// The swipe of the one contact that travelled, ignoring contacts that
    /// stayed put.
    ///
    /// A finger swiping while the writing hand rests on the sheet raises the
    /// frame's finger count to two, and the gesture used to be dropped on that
    /// alone — the reason page turns felt unreliable in the hand rather than on
    /// the bench. What distinguishes a palm is that it does not travel, so ask
    /// that instead of counting contacts. Two travellers are a real two-finger
    /// drag and belong to Scroll; a contact that smeared somewhere between
    /// resting and swiping makes the whole gesture ambiguous, and ambiguous
    /// input does nothing.
    fn lone_swipe(&self) -> Option<Gesture> {
        let mut traveller = None;
        for slot in
            self.slots.iter().filter(|s| s.start_x != i32::MIN && s.start_y != i32::MIN)
        {
            let (x0, y0) = map_touch(slot.start_x, slot.start_y);
            let (x1, y1) = map_touch(slot.x, slot.y);
            let travel = (x1 - x0).abs() + (y1 - y0).abs();
            if travel >= SWIPE_PX {
                if traveller.is_some() {
                    return None;
                }
                traveller = Some((x0, y0, x1, y1));
            } else if travel >= TAP_SLOP {
                return None;
            }
        }
        traveller.and_then(|(x0, y0, x1, y1)| classify_swipe(x0, y0, x1, y1))
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

/// `None` when the movement is real but means nothing — short of a page's
/// worth of travel, or more sideways than vertical. The old catch-all made
/// every such contact a page flip, which is how noise reached the notebook.
fn classify_swipe(x0: i32, y0: i32, x1: i32, y1: i32) -> Option<Gesture> {
    let (dx, dy) = (x1 - x0, y1 - y0);
    if x0 <= EDGE_PX && dx >= SWIPE_PX && dx.abs() > dy.abs() {
        Some(Gesture::OpenDrawer)
    } else if dx <= -SWIPE_PX && dx.abs() > dy.abs() {
        Some(Gesture::CloseDrawer)
    } else if y0 <= EDGE_PX && dy >= SWIPE_PX && dy.abs() > dx.abs() {
        Some(Gesture::OpenControls)
    } else if y0 >= fb::SCREEN_H as i32 - EDGE_PX && dy <= -SWIPE_PX && dy.abs() > dx.abs() {
        Some(Gesture::OpenControls)
    } else if dy.abs() >= PAGE_PX && dy.abs() > dx.abs() {
        Some(Gesture::Page((-dy).signum()))
    } else {
        None
    }
}

/// Classify screen-space points supplied by window-system touch fallback.
pub fn gesture_from_points(start: (i32, i32), end: (i32, i32)) -> Option<Gesture> {
    if (end.0 - start.0).abs() + (end.1 - start.1).abs() < TAP_SLOP {
        Some(Gesture::Tap(end.0, end.1))
    } else {
        classify_swipe(start.0, start.1, end.0, end.1)
    }
}

fn five_finger_release_is_quit(max_fingers: usize, hold_frames: usize, motion: i32) -> bool {
    max_fingers >= 5 && hold_frames >= FIVE_FINGER_HOLD_FRAMES && motion < TAP_SLOP
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_device() -> TouchDevice {
        TouchDevice {
            fd: -1,
            slots: [Slot::default(); MAX_SLOTS],
            cur: 0,
            max_fingers: 0,
            frame_x: None,
            frame_y: None,
            total_motion: 0,
            five_finger_hold_frames: 0,
            frame_fingers: 0,
        }
    }

    fn land(dev: &mut TouchDevice, slot: usize, x: i32, y: i32, out: &mut Vec<Gesture>) {
        dev.slots[slot] = Slot { active: true, start_x: x, start_y: y, x, y };
        dev.finish_frame(out);
    }

    // Fingers never land or lift simultaneously. The averaged contact point
    // jumps by hundreds of raw units as each finger joins or leaves, and that
    // jump must not count as motion, or a real hand can never hold still
    // enough to quit.
    #[test]
    fn five_fingers_landing_one_per_frame_still_quit() {
        let mut dev = test_device();
        let mut out = Vec::new();
        let xs = [200, 500, 800, 1100, 1300];
        for (i, &x) in xs.iter().enumerate() {
            land(&mut dev, i, x, 900 + i as i32 * 40, &mut out);
        }
        for _ in 0..FIVE_FINGER_HOLD_FRAMES + 5 {
            dev.finish_frame(&mut out);
        }
        for i in 0..5 {
            dev.slots[i].active = false;
            dev.finish_frame(&mut out);
        }
        assert!(out.contains(&Gesture::Quit), "gestures were {out:?}");
    }

    #[test]
    fn five_fingers_dragging_do_not_quit() {
        let mut dev = test_device();
        let mut out = Vec::new();
        for i in 0..5 {
            land(&mut dev, i, 200 + i as i32 * 250, 900, &mut out);
        }
        for step in 0..FIVE_FINGER_HOLD_FRAMES + 5 {
            for i in 0..5 {
                dev.slots[i].y = 900 + step as i32 * 8;
            }
            dev.finish_frame(&mut out);
        }
        for i in 0..5 {
            dev.slots[i].active = false;
            dev.finish_frame(&mut out);
        }
        assert!(!out.contains(&Gesture::Quit), "gestures were {out:?}");
    }

    #[test]
    fn five_finger_quit_requires_a_stationary_hold() {
        assert!(!five_finger_release_is_quit(5, FIVE_FINGER_HOLD_FRAMES - 1, 0));
        assert!(!five_finger_release_is_quit(5, FIVE_FINGER_HOLD_FRAMES, TAP_SLOP));
        assert!(five_finger_release_is_quit(5, FIVE_FINGER_HOLD_FRAMES, TAP_SLOP - 1));
    }

    #[test]
    fn edge_swipe_is_reserved_for_drawer_not_page_navigation() {
        assert_eq!(classify_swipe(20, 500, 240, 510), Some(Gesture::OpenDrawer));
        assert_eq!(classify_swipe(200, 500, 210, 250), Some(Gesture::Page(1)));
        assert_eq!(classify_swipe(300, 500, 100, 510), Some(Gesture::CloseDrawer));
        assert_eq!(classify_swipe(200, 10, 210, 200), Some(Gesture::OpenControls));
        assert_eq!(
            classify_swipe(200, fb::SCREEN_H as i32 - 10, 210, fb::SCREEN_H as i32 - 200),
            Some(Gesture::OpenControls)
        );
    }

    // The catch-all that used to end classify_swipe made every one-finger
    // contact a page flip the moment its jitter cleared TAP_SLOP. A hand
    // resting on the sheet turned the notebook by itself.
    #[test]
    fn short_or_sideways_travel_is_not_a_page_flip() {
        assert_eq!(classify_swipe(700, 900, 706, 830), None, "jitter is not a flip");
        assert_eq!(classify_swipe(700, 900, 900, 905), None, "sideways is not a flip");
        assert_eq!(
            classify_swipe(700, 900, 760, 900 - PAGE_PX + 1),
            None,
            "just short of a page's travel is not a flip"
        );
        assert_eq!(
            classify_swipe(700, 900, 706, 900 - PAGE_PX),
            Some(Gesture::Page(1)),
            "a deliberate upward swipe still flips forward"
        );
    }

    // The writing hand rests on the sheet while a finger swipes. That is two
    // contacts, and the gesture used to be dropped for it.
    #[test]
    fn a_finger_swipe_survives_a_resting_palm() {
        let mut dev = test_device();
        let mut out = Vec::new();
        land(&mut dev, 0, 300, 400, &mut out);          // the palm, and it stays
        land(&mut dev, 1, 900, 1400, &mut out);         // the finger, about to travel
        for step in 1..=10 {
            dev.slots[1].y = 1400 - step * 60;
            dev.finish_frame(&mut out);
        }
        dev.slots[0].active = false;
        dev.slots[1].active = false;
        dev.finish_frame(&mut out);
        assert!(
            out.iter().any(|g| matches!(g, Gesture::Page(_))),
            "gestures were {out:?}"
        );
    }

    // Two contacts that both travel are a two-finger drag; Scroll already
    // reported it frame by frame and a page flip on top would double-count.
    #[test]
    fn a_two_finger_drag_is_not_also_a_page_flip() {
        let mut dev = test_device();
        let mut out = Vec::new();
        land(&mut dev, 0, 600, 1400, &mut out);
        land(&mut dev, 1, 900, 1400, &mut out);
        for step in 1..=10 {
            dev.slots[0].y = 1400 - step * 60;
            dev.slots[1].y = 1400 - step * 60;
            dev.finish_frame(&mut out);
        }
        dev.slots[0].active = false;
        dev.slots[1].active = false;
        dev.finish_frame(&mut out);
        assert!(!out.iter().any(|g| matches!(g, Gesture::Page(_))), "gestures were {out:?}");
    }

    // Slot coordinates outlive the gesture that set them. Without a wipe, the
    // next contact is measured against a stranger's starting point.
    #[test]
    fn a_finished_gesture_leaves_no_coordinates_behind() {
        let mut dev = test_device();
        let mut out = Vec::new();
        land(&mut dev, 0, 900, 1400, &mut out);
        for step in 1..=10 {
            dev.slots[0].y = 1400 - step * 60;
            dev.finish_frame(&mut out);
        }
        dev.slots[0].active = false;
        dev.finish_frame(&mut out);
        assert!(
            dev.slots.iter().all(|s| s.start_x == i32::MIN || !s.active),
            "a released slot kept its start"
        );
        assert_eq!(dev.lone_swipe(), None, "a spent gesture still reads as a swipe");
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
