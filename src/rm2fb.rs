//! rm2fb takeover client for the reMarkable 2.
//!
//! In takeover mode xochitl is stopped and the panel is driven by the
//! rm2display server (timower/rM2-stuff), which runs the vendor epaper
//! engine (libqsgepaper) standalone. riddle draws into the server's shared
//! RGB565 buffer and requests refreshes over a unix datagram socket — with
//! a per-update WAVEFORM, which is the whole point: DU ink lands with the
//! pen, GC16 dries it to crisp black. Windowed qtfb can never offer that.
//!
//! Protocol (libs/rm2fb in timower/rM2-stuff):
//!   - shared fb:  /dev/shm/swtfb.01 — 1404×1872 u16 RGB565 (+ gray aux)
//!   - control:    /var/run/rm2fb.sock — sendto(UpdateParams), no ack
//!   - UpdateParams coordinates are INCLUSIVE; a 1×1 rect is treated as a
//!     liveness ping by the server, so updates are padded to ≥2×2.
//!
//! Both paths are env-overridable (RM2FB_SHM_PATH / RM2FB_SOCK) so the
//! whole backend can run against a viewer on a dev machine.

use std::io;
use std::os::fd::{AsRawFd, OwnedFd};

pub const FB_W: usize = 1404;
pub const FB_H: usize = 1872;
const FB_BYTES: usize = FB_W * FB_H * 2;
// The server's mapping also carries an 8-bit gray buffer after the fb.
const TOTAL_BYTES: usize = FB_BYTES + FB_W * FB_H;

/// mxcfb-style waveform numbers, marked with the flag the server's
/// mapWaveform() recognizes and translates to engine-internal values.
const IOCTL_WAVEFORM_FLAG: i32 = 0xf000;
pub const WAVE_DU: i32 = IOCTL_WAVEFORM_FLAG | 1; // fast 2-level: live ink
pub const WAVE_GC16: i32 = IOCTL_WAVEFORM_FLAG | 2; // 16-level: crisp black

#[repr(C)]
struct UpdateParams {
    y1: i32,
    x1: i32,
    y2: i32,
    x2: i32,
    flags: i32,
    waveform: i32,
    temperature_override: f32,
    extra_mode: i32,
}

pub struct Rm2fbClient {
    sock: OwnedFd,
    sock_addr: libc::sockaddr_un,
    sock_len: libc::socklen_t,
    ptr: *mut u8,
}

impl Rm2fbClient {
    pub fn open() -> io::Result<Self> {
        let shm_path =
            std::env::var("RM2FB_SHM_PATH").unwrap_or_else(|_| "/dev/shm/swtfb.01".into());
        let sock_path =
            std::env::var("RM2FB_SOCK").unwrap_or_else(|_| "/var/run/rm2fb.sock".into());

        // The shm object is a plain file under /dev/shm; open+mmap is
        // equivalent to shm_open and works for the dev-machine override too.
        let cpath = std::ffi::CString::new(shm_path.clone()).unwrap();
        let fd = unsafe { libc::open(cpath.as_ptr(), libc::O_RDWR | libc::O_CREAT, 0o666) };
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe { libc::ftruncate(fd, TOTAL_BYTES as libc::off_t) } != 0 {
            let e = io::Error::last_os_error();
            unsafe { libc::close(fd) };
            return Err(e);
        }
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                TOTAL_BYTES,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        unsafe { libc::close(fd) };
        if ptr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }

        let sock = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_DGRAM, 0) };
        if sock < 0 {
            return Err(io::Error::last_os_error());
        }
        let sock = unsafe { <OwnedFd as std::os::fd::FromRawFd>::from_raw_fd(sock) };
        let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
        addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
        let bytes = sock_path.as_bytes();
        if bytes.len() >= addr.sun_path.len() {
            return Err(io::Error::other("rm2fb socket path too long"));
        }
        for (dst, src) in addr.sun_path.iter_mut().zip(bytes) {
            *dst = *src as libc::c_char;
        }
        let sock_len = (std::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1)
            as libc::socklen_t;

        let client = Self { sock, sock_addr: addr, sock_len, ptr: ptr as *mut u8 };
        eprintln!("g-pad: rm2fb takeover — fb {shm_path}, control {sock_path}");
        Ok(client)
    }

    pub fn framebuffer(&self) -> (*mut u8, usize) {
        (self.ptr, FB_BYTES)
    }

    /// Ask the server to refresh a region with the given waveform. Clamped,
    /// inclusive coordinates, padded past the server's 1×1 ping heuristic.
    pub fn update(&self, x: i32, y: i32, w: i32, h: i32, waveform: i32) {
        let x1 = x.clamp(0, FB_W as i32 - 1);
        let y1 = y.clamp(0, FB_H as i32 - 1);
        let mut x2 = (x + w - 1).clamp(x1, FB_W as i32 - 1);
        let mut y2 = (y + h - 1).clamp(y1, FB_H as i32 - 1);
        if x2 == x1 {
            x2 = (x1 + 1).min(FB_W as i32 - 1);
        }
        if y2 == y1 {
            y2 = (y1 + 1).min(FB_H as i32 - 1);
        }
        let params = UpdateParams {
            y1,
            x1,
            y2,
            x2,
            flags: 0,
            waveform,
            temperature_override: 0.0,
            extra_mode: 0,
        };
        let sent = unsafe {
            libc::sendto(
                self.sock.as_raw_fd(),
                &params as *const UpdateParams as *const libc::c_void,
                std::mem::size_of::<UpdateParams>(),
                0,
                &self.sock_addr as *const libc::sockaddr_un as *const libc::sockaddr,
                self.sock_len,
            )
        };
        if sent < 0 {
            // The server going away mid-session is survivable: log, don't die.
            eprintln!("g-pad: rm2fb update failed: {}", io::Error::last_os_error());
        }
    }
}

impl Drop for Rm2fbClient {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, TOTAL_BYTES) };
    }
}
