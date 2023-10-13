#![allow(clippy::unreadable_literal)]

use std::ffi::CString;
use std::os::unix::io::RawFd;
use std::ptr::null_mut;
use std::{io, mem, usize};

// C types and constants.
use libc::timeval as Timeval;
use libc::{c_ulong, c_void, off_t, size_t};
use libc::{MAP_SHARED, O_RDWR, PROT_READ, PROT_WRITE};

#[cfg(not(feature = "no_wrapper"))]
mod ll {
    use libc::{c_char, c_int, c_ulong, c_void, off_t, size_t};
    use std::os::unix::io::RawFd;

    pub use self::v4l2_close as close;
    pub use self::v4l2_ioctl as ioctl;
    pub use self::v4l2_munmap as munmap;
    pub use self::v4l2_open as open;

    pub unsafe fn mmap(
        start: *mut c_void,
        length: size_t,
        prot: c_int,
        flags: c_int,
        fd: RawFd,
        offset: off_t,
    ) -> *mut c_void {
        // Note the subtle function signature mismatch between mmap and v4l2_mmap.
        v4l2_mmap(start, length, prot, flags, fd, offset as i64)
    }

    #[link(name = "v4l2")]
    extern "C" {
        pub fn v4l2_open(file: *const c_char, flags: c_int, arg: c_int) -> RawFd;
        pub fn v4l2_close(fd: RawFd) -> c_int;
        pub fn v4l2_ioctl(fd: RawFd, request: c_ulong, argp: *mut c_void) -> c_int;
        pub fn v4l2_mmap(
            start: *mut c_void,
            length: size_t,
            prot: c_int,
            flags: c_int,
            fd: RawFd,
            offset: i64,
        ) -> *mut c_void;
        pub fn v4l2_munmap(start: *mut c_void, length: size_t) -> c_int;
    }
}

#[cfg(feature = "no_wrapper")]
mod ll {
    use libc::{c_int, c_ulong, c_void, off_t, size_t};
    use std::os::unix::io::RawFd;

    pub use libc::{close, munmap, open};

    pub unsafe fn mmap(
        start: *mut c_void,
        length: size_t,
        prot: c_int,
        flags: c_int,
        fd: RawFd,
        offset: off_t,
    ) -> *mut c_void {
        libc::mmap(start, length, prot, flags, fd, offset)
    }

    extern "C" {
        pub fn ioctl(fd: RawFd, request: c_ulong, argp: *mut c_void) -> c_int;
    }
}

macro_rules! check_io(
    ($cond:expr) =>
        (if $cond { Ok(()) } else { Err(io::Error::last_os_error()) }?)
);

pub fn open(file: &str) -> io::Result<RawFd> {
    let c_str = CString::new(file)?;
    let fd = unsafe { ll::open(c_str.as_ptr(), O_RDWR, 0) };
    check_io!(fd != -1);
    Ok(fd)
}

pub fn close(fd: RawFd) -> io::Result<()> {
    check_io!(unsafe { ll::close(fd) != -1 });
    Ok(())
}

pub fn xioctl<T>(fd: RawFd, request: usize, arg: &mut T) -> io::Result<()> {
    let argp: *mut T = arg;

    check_io!(unsafe {
        let mut ok;

        loop {
            ok = ll::ioctl(fd, request as c_ulong, argp as *mut c_void) != -1;
            if ok || io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                break;
            }
        }

        ok
    });

    Ok(())
}

pub fn xioctl_valid<T>(fd: RawFd, request: usize, arg: &mut T) -> io::Result<bool> {
    match xioctl(fd, request, arg) {
        Ok(_) => Ok(true),
        Err(ref err) if err.kind() == io::ErrorKind::InvalidInput => Ok(false),
        Err(err) => Err(err),
    }
}

pub struct MappedRegion {
    pub ptr: *mut u8,
    pub len: usize,
}

// Instead of unstable `Unique<u8>`.
unsafe impl Send for MappedRegion {}
unsafe impl Sync for MappedRegion {}

impl Drop for MappedRegion {
    fn drop(&mut self) {
        unsafe {
            ll::munmap(self.ptr as *mut c_void, self.len as size_t);
        }
    }
}

pub fn mmap(length: usize, fd: RawFd, offset: usize) -> io::Result<MappedRegion> {
    let ptr = unsafe {
        ll::mmap(
            null_mut(),
            length as size_t,
            PROT_READ | PROT_WRITE,
            MAP_SHARED,
            fd,
            offset as off_t,
        )
    };

    check_io!(ptr as usize != usize::MAX);
    Ok(MappedRegion {
        ptr: ptr as *mut u8,
        len: length,
    })
}

#[repr(C)]
pub struct Format {
    pub ftype: u32,
    #[cfg(target_pointer_width = "64")]
    padding: u32,
    pub fmt: PixFormat,
    space: [u8; 156],
}

impl Format {
    #[cfg(target_pointer_width = "64")]
    pub fn new(resolution: (u32, u32), fourcc: u32, field: u32) -> Format {
        Format {
            ftype: BUF_TYPE_VIDEO_CAPTURE,
            padding: 0,
            fmt: PixFormat::new(resolution, fourcc, field),
            space: [0; 156],
        }
    }

    #[cfg(target_pointer_width = "32")]
    pub fn new(resolution: (u32, u32), fourcc: u32, field: u32) -> Format {
        Format {
            ftype: BUF_TYPE_VIDEO_CAPTURE,
            fmt: PixFormat::new(resolution, fourcc, field),
            space: [0; 156],
        }
    }
}

#[repr(C)]
pub struct PixFormat {
    pub width: u32,
    pub height: u32,
    pub pixelformat: u32,
    pub field: u32,
    pub bytesperline: u32,
    pub sizeimage: u32,
    pub colorspace: u32,
    pub private: u32,
    pub flags: u32,
    pub ycbcr_enc: u32,
    pub quantization: u32,
}

impl PixFormat {
    pub fn new(resolution: (u32, u32), fourcc: u32, field: u32) -> PixFormat {
        let mut pix_fmt: PixFormat = unsafe { mem::zeroed() };
        pix_fmt.width = resolution.0;
        pix_fmt.height = resolution.1;
        pix_fmt.pixelformat = fourcc;
        pix_fmt.field = field;
        pix_fmt
    }
}

#[repr(C)]
pub struct RequestBuffers {
    pub count: u32,
    pub btype: u32,
    pub memory: u32,
    reserved: [u32; 2],
}

impl RequestBuffers {
    pub fn new(nbuffers: u32) -> RequestBuffers {
        RequestBuffers {
            count: nbuffers,
            btype: BUF_TYPE_VIDEO_CAPTURE,
            memory: MEMORY_MMAP,
            reserved: [0; 2],
        }
    }
}

#[repr(C)]
pub struct Buffer {
    pub index: u32,
    pub btype: u32,
    pub bytesused: u32,
    pub flags: u32,
    pub field: u32,
    pub timestamp: Timeval,
    pub timecode: TimeCode,
    pub sequence: u32,
    pub memory: u32,
    pub m: usize, // offset (__u32) or userptr (ulong)
    pub length: u32,
    pub input: u32,
    reserved: u32,
}

impl Buffer {
    pub fn new() -> Buffer {
        let mut buf: Buffer = unsafe { mem::zeroed() };
        buf.btype = BUF_TYPE_VIDEO_CAPTURE;
        buf.memory = MEMORY_MMAP;
        buf
    }
}

#[repr(C)]
pub struct TimeCode {
    pub ttype: u32,
    pub flags: u32,
    pub frames: u8,
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub userbits: [u8; 4],
}

#[repr(C)]
pub struct FmtDesc {
    pub index: u32,
    pub ftype: u32,
    pub flags: u32,
    pub description: [u8; 32],
    pub pixelformat: u32,
    reserved: [u32; 4],
}

impl FmtDesc {
    pub fn new() -> FmtDesc {
        let mut desc: FmtDesc = unsafe { mem::zeroed() };
        desc.ftype = BUF_TYPE_VIDEO_CAPTURE;
        desc
    }
}

#[repr(C)]
pub struct StreamParm {
    pub ptype: u32,
    pub parm: CaptureParm,
    space: [u8; 160],
}

impl StreamParm {
    pub fn new(interval: (u32, u32)) -> StreamParm {
        let mut parm: StreamParm = unsafe { mem::zeroed() };
        parm.ptype = BUF_TYPE_VIDEO_CAPTURE;
        parm.parm.timeperframe.numerator = interval.0;
        parm.parm.timeperframe.denominator = interval.1;
        parm
    }
}

#[repr(C)]
pub struct CaptureParm {
    pub capability: u32,
    pub capturemode: u32,
    pub timeperframe: Fract,
    pub extendedmode: u32,
    pub readbuffers: u32,
    reserved: [u32; 4],
}

#[repr(C)]
pub struct Fract {
    pub numerator: u32,
    pub denominator: u32,
}

#[repr(C)]
pub struct Frmsizeenum {
    pub index: u32,
    pub pixelformat: u32,
    pub ftype: u32,
    data: [u32; 6],
    reserved: [u32; 2],
}

impl Frmsizeenum {
    pub fn new(fourcc: u32) -> Frmsizeenum {
        let mut size: Frmsizeenum = unsafe { mem::zeroed() };
        size.pixelformat = fourcc;
        size
    }

    pub fn discrete(&mut self) -> &mut FrmsizeDiscrete {
        unsafe { &mut *(self.data.as_mut_ptr() as *mut FrmsizeDiscrete) }
    }

    pub fn stepwise(&mut self) -> &mut FrmsizeStepwise {
        unsafe { &mut *(self.data.as_mut_ptr() as *mut FrmsizeStepwise) }
    }
}

#[repr(C)]
pub struct FrmsizeDiscrete {
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
pub struct FrmsizeStepwise {
    pub min_width: u32,
    pub max_width: u32,
    pub step_width: u32,
    pub min_height: u32,
    pub max_height: u32,
    pub step_height: u32,
}

#[repr(C)]
pub struct Frmivalenum {
    pub index: u32,
    pub pixelformat: u32,
    pub width: u32,
    pub height: u32,
    pub ftype: u32,
    data: [u32; 6],
    reserved: [u32; 2],
}

impl Frmivalenum {
    pub fn new(fourcc: u32, resolution: (u32, u32)) -> Frmivalenum {
        let mut ival: Frmivalenum = unsafe { mem::zeroed() };
        ival.pixelformat = fourcc;
        ival.width = resolution.0;
        ival.height = resolution.1;
        ival
    }

    pub fn discrete(&mut self) -> &mut Fract {
        unsafe { &mut *(self.data.as_mut_ptr() as *mut Fract) }
    }

    pub fn stepwise(&mut self) -> &mut FrmivalStepwise {
        unsafe { &mut *(self.data.as_mut_ptr() as *mut FrmivalStepwise) }
    }
}

#[repr(C)]
pub struct FrmivalStepwise {
    pub min: Fract,
    pub max: Fract,
    pub step: Fract,
}

#[repr(C)]
pub struct QueryCtrl {
    pub id: u32,
    pub qtype: u32,
    pub name: [u8; 32],
    pub minimum: i32,
    pub maximum: i32,
    pub step: i32,
    pub default_value: i32,
    pub flags: u32,
    reserved: [u32; 2],
}

impl QueryCtrl {
    pub fn new(id: u32) -> QueryCtrl {
        let mut qctrl: QueryCtrl = unsafe { mem::zeroed() };
        qctrl.id = id;
        qctrl
    }
}

#[repr(C)]
pub struct QueryExtCtrl {
    pub id: u32,
    pub qtype: u32,
    pub name: [u8; 32],
    pub minimum: i64,
    pub maximum: i64,
    pub step: u64,
    pub default_value: i64,
    pub flags: u32,
    pub elem_size: u32,
    pub elems: u32,
    pub nr_of_dims: u32,
    pub dims: [u32; 4],
    reserved: [u32; 32],
}

impl QueryExtCtrl {
    pub fn new(id: u32) -> QueryExtCtrl {
        let mut qctrl: QueryExtCtrl = unsafe { mem::zeroed() };
        qctrl.id = id;
        qctrl.elem_size = 8;
        qctrl.elems = 1;
        qctrl
    }
}

#[repr(C, packed)]
pub struct QueryMenu {
    pub id: u32,
    pub index: u32,
    pub data: QueryMenuData,
    reserved: u32,
}

#[repr(C, packed)]
pub union QueryMenuData {
    name: [u8; 32],
    value: i64,
}

impl QueryMenu {
    pub fn new(id: u32) -> QueryMenu {
        let mut menu: QueryMenu = unsafe { mem::zeroed() };
        menu.id = id;
        menu
    }
}

impl QueryMenuData {
    pub fn name(&self) -> &[u8] {
        unsafe { &self.name[..] }
    }

    pub fn value(&self) -> i64 {
        unsafe { self.value }
    }
}

#[repr(C)]
pub struct Control {
    pub id: u32,
    pub value: i32,
}

impl Control {
    pub fn new(id: u32) -> Control {
        Control { id, value: 0 }
    }
}

#[repr(C, packed)]
pub struct ExtControl {
    pub id: u32,
    pub size: u32,
    reserved: u32,
    pub value: i64,
}

impl ExtControl {
    pub fn new(id: u32, size: u32) -> ExtControl {
        ExtControl {
            id,
            size,
            reserved: 0,
            value: 0,
        }
    }
}

#[repr(C)]
pub struct ExtControls<'a> {
    pub ctrl_class: u32,
    pub count: u32,
    pub error_idx: u32,
    reserved: [u32; 2],
    pub controls: &'a mut ExtControl,
}

impl<'a> ExtControls<'a> {
    pub fn new(class: u32, ctrl: &mut ExtControl) -> ExtControls<'_> {
        ExtControls {
            ctrl_class: class,
            count: 1,
            error_idx: 0,
            reserved: [0; 2],
            controls: ctrl,
        }
    }
}

pub const BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
pub const FMT_FLAG_COMPRESSED: u32 = 1;
pub const FMT_FLAG_EMULATED: u32 = 2;
pub const FRMIVAL_TYPE_DISCRETE: u32 = 1;
pub const FRMSIZE_TYPE_DISCRETE: u32 = 1;
pub const MEMORY_MMAP: u32 = 1;

pub const ID2CLASS: u32 = 0x0fff0000;
pub const NEXT_CTRL: u32 = 0x80000000;

// Control types.
pub const CTRL_TYPE_INTEGER: u32 = 1;
pub const CTRL_TYPE_BOOLEAN: u32 = 2;
pub const CTRL_TYPE_MENU: u32 = 3;
pub const CTRL_TYPE_BUTTON: u32 = 4;
pub const CTRL_TYPE_INTEGER64: u32 = 5;
pub const CTRL_TYPE_CTRL_CLASS: u32 = 6;
pub const CTRL_TYPE_STRING: u32 = 7;
pub const CTRL_TYPE_BITMASK: u32 = 8;
pub const CTRL_TYPE_INTEGER_MENU: u32 = 9;

#[allow(non_upper_case_globals)]
pub mod pubconsts {
    // Fields.
    /// None, top, bottom or interplaced depending on whatever it thinks is approximate.
    pub const FIELD_ANY: u32 = 0;
    /// This device has no fields.
    pub const FIELD_NONE: u32 = 1;
    /// Top field only.
    pub const FIELD_TOP: u32 = 2;
    /// Bottom field only.
    pub const FIELD_BOTTOM: u32 = 3;
    /// Both fields interplaced.
    pub const FIELD_INTERLACED: u32 = 4;
    /// Both fields sequential into one buffer, top-bottom order.
    pub const FIELD_SEQ_TB: u32 = 5;
    /// Both fields sequential into one buffer, bottom-top order.
    pub const FIELD_SEQ_BT: u32 = 6;
    /// Both fields alternating into separate buffers.
    pub const FIELD_ALTERNATE: u32 = 7;
    /// Both fields interplaced, top field first and the top field is transmitted first.
    pub const FIELD_INTERLACED_TB: u32 = 8;
    /// Both fields interplaced, top field first and the bottom field is transmitted first.
    pub const FIELD_INTERLACED_BT: u32 = 9;

    // Control flags.
    /// This control is permanently disabled and should be ignored by the application.
    pub const FLAG_DISABLED: u32 = 0x0001;
    /// This control is temporarily unchangeable (e.g. another application controls resource).
    pub const FLAG_GRABBED: u32 = 0x0002;
    /// This control is permanently readable only.
    pub const FLAG_READ_ONLY: u32 = 0x0004;
    /// Changing this control may affect the value of other controls within the same control class.
    pub const FLAG_UPDATE: u32 = 0x0008;
    /// This control is not applicable to the current configuration.
    pub const FLAG_INACTIVE: u32 = 0x0010;
    /// A hint that this control is best represented as a slider-like element in a user interface.
    pub const FLAG_SLIDER: u32 = 0x0020;
    /// This control is permanently writable only.
    pub const FLAG_WRITE_ONLY: u32 = 0x0040;
    /// This control is volatile, which means that the value of the control changes continuously.
    /// A typical example would be the current gain value if the device is in auto-gain mode.
    pub const FLAG_VOLATILE: u32 = 0x0080;
    /// This control has a pointer type.
    pub const FLAG_HAS_PAYLOAD: u32 = 0x0100;
    /// The value provided to the control will be propagated to the driver even if it remains
    /// constant. This is required when the control represents an action on the hardware.
    /// For example: clearing an error flag or triggering the flash.
    pub const FLAG_EXECUTE_ON_WRITE: u32 = 0x0200;

    // Control classses.
    /// User controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/control.html).
    pub const CLASS_USER: u32 = 0x00980000;
    /// MPEG compression controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#mpeg-controls).
    pub const CLASS_MPEG: u32 = 0x00990000;
    /// Camera controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#camera-controls).
    pub const CLASS_CAMERA: u32 = 0x009a0000;
    /// FM Transmitter controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#fm-tx-controls).
    pub const CLASS_FM_TX: u32 = 0x009b0000;
    /// Flash device controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#flash-controls).
    pub const CLASS_FLASH: u32 = 0x009c0000;
    /// JPEG compression controls.
    /// [details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#jpeg-controls).
    pub const CLASS_JPEG: u32 = 0x009d0000;
    /// low-level controls of image source.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#image-source-controls).
    pub const CLASS_IMAGE_SOURCE: u32 = 0x009e0000;
    /// Image processing controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#image-process-controls).
    pub const CLASS_IMAGE_PROC: u32 = 0x009f0000;
    /// Digital Video controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#dv-controls).
    pub const CLASS_DV: u32 = 0x00a00000;
    /// FM Receiver controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#fm-rx-controls).
    pub const CLASS_FM_RX: u32 = 0x00a10000;
    /// RF tuner controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#rf-tuner-controls).
    pub const CLASS_RF_TUNER: u32 = 0x00a20000;
    /// Motion or object detection controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/uapi/v4l/extended-controls.html#detect-controls).
    pub const CLASS_DETECT: u32 = 0x00a30000;

    pub const CID_BASE: u32 = CLASS_USER | 0x900;
    pub const CID_USER_BASE: u32 = CID_BASE;
    pub const CID_USER_CLASS: u32 = CLASS_USER | 1;
    pub const CID_BRIGHTNESS: u32 = CID_BASE;
    pub const CID_CONTRAST: u32 = CID_BASE + 1;
    pub const CID_SATURATION: u32 = CID_BASE + 2;
    pub const CID_HUE: u32 = CID_BASE + 3;
    pub const CID_AUDIO_VOLUME: u32 = CID_BASE + 5;
    pub const CID_AUDIO_BALANCE: u32 = CID_BASE + 6;
    pub const CID_AUDIO_BASS: u32 = CID_BASE + 7;
    pub const CID_AUDIO_TREBLE: u32 = CID_BASE + 8;
    pub const CID_AUDIO_MUTE: u32 = CID_BASE + 9;
    pub const CID_AUDIO_LOUDNESS: u32 = CID_BASE + 10;
    pub const CID_BLACK_LEVEL: u32 = CID_BASE + 11;
    pub const CID_AUTO_WHITE_BALANCE: u32 = CID_BASE + 12;
    pub const CID_DO_WHITE_BALANCE: u32 = CID_BASE + 13;
    pub const CID_RED_BALANCE: u32 = CID_BASE + 14;
    pub const CID_BLUE_BALANCE: u32 = CID_BASE + 15;
    pub const CID_GAMMA: u32 = CID_BASE + 16;
    pub const CID_WHITENESS: u32 = CID_GAMMA;
    pub const CID_EXPOSURE: u32 = CID_BASE + 17;
    pub const CID_AUTOGAIN: u32 = CID_BASE + 18;
    pub const CID_GAIN: u32 = CID_BASE + 19;
    pub const CID_HFLIP: u32 = CID_BASE + 20;
    pub const CID_VFLIP: u32 = CID_BASE + 21;
    pub const CID_POWER_LINE_FREQUENCY: u32 = CID_BASE + 24;
    pub const CID_POWER_LINE_FREQUENCY_DISABLED: u32 = 0;
    pub const CID_POWER_LINE_FREQUENCY_50HZ: u32 = 1;
    pub const CID_POWER_LINE_FREQUENCY_60HZ: u32 = 2;
    pub const CID_POWER_LINE_FREQUENCY_AUTO: u32 = 3;
    pub const CID_HUE_AUTO: u32 = CID_BASE + 25;
    pub const CID_WHITE_BALANCE_TEMPERATURE: u32 = CID_BASE + 26;
    pub const CID_SHARPNESS: u32 = CID_BASE + 27;
    pub const CID_BACKLIGHT_COMPENSATION: u32 = CID_BASE + 28;
    pub const CID_CHROMA_AGC: u32 = CID_BASE + 29;
    pub const CID_COLOR_KILLER: u32 = CID_BASE + 30;
    pub const CID_COLORFX: u32 = CID_BASE + 31;
    pub const COLORFX_NONE: u32 = 0;
    pub const COLORFX_BW: u32 = 1;
    pub const COLORFX_SEPIA: u32 = 2;
    pub const COLORFX_NEGATIVE: u32 = 3;
    pub const COLORFX_EMBOSS: u32 = 4;
    pub const COLORFX_SKETCH: u32 = 5;
    pub const COLORFX_SKY_BLUE: u32 = 6;
    pub const COLORFX_GRASS_GREEN: u32 = 7;
    pub const COLORFX_SKIN_WHITEN: u32 = 8;
    pub const COLORFX_VIVID: u32 = 9;
    pub const COLORFX_AQUA: u32 = 10;
    pub const COLORFX_ART_FREEZE: u32 = 11;
    pub const COLORFX_SILHOUETTE: u32 = 12;
    pub const COLORFX_SOLARIZATION: u32 = 13;
    pub const COLORFX_ANTIQUE: u32 = 14;
    pub const COLORFX_SET_CBCR: u32 = 15;
    pub const CID_AUTOBRIGHTNESS: u32 = CID_BASE + 32;
    pub const CID_BAND_STOP_FILTER: u32 = CID_BASE + 33;
    pub const CID_ROTATE: u32 = CID_BASE + 34;
    pub const CID_BG_COLOR: u32 = CID_BASE + 35;
    pub const CID_CHROMA_GAIN: u32 = CID_BASE + 36;
    pub const CID_ILLUMINATORS_1: u32 = CID_BASE + 37;
    pub const CID_ILLUMINATORS_2: u32 = CID_BASE + 38;
    pub const CID_MIN_BUFFERS_FOR_CAPTURE: u32 = CID_BASE + 39;
    pub const CID_MIN_BUFFERS_FOR_OUTPUT: u32 = CID_BASE + 40;
    pub const CID_ALPHA_COMPONENT: u32 = CID_BASE + 41;
    pub const CID_COLORFX_CBCR: u32 = CID_BASE + 42;
    pub const CID_LASTP1: u32 = CID_BASE + 43;
    pub const CID_USER_MEYE_BASE: u32 = CID_USER_BASE + 0x1000;
    pub const CID_USER_BTTV_BASE: u32 = CID_USER_BASE + 0x1010;
    pub const CID_USER_S2255_BASE: u32 = CID_USER_BASE + 0x1030;
    pub const CID_USER_SI476X_BASE: u32 = CID_USER_BASE + 0x1040;
    pub const CID_USER_TI_VPE_BASE: u32 = CID_USER_BASE + 0x1050;
    pub const CID_USER_SAA7134_BASE: u32 = CID_USER_BASE + 0x1060;
    pub const CID_USER_ADV7180_BASE: u32 = CID_USER_BASE + 0x1070;
    pub const CID_MPEG_BASE: u32 = CLASS_MPEG | 0x900;
    pub const CID_MPEG_CLASS: u32 = CLASS_MPEG | 1;
    pub const CID_MPEG_STREAM_TYPE: u32 = CID_MPEG_BASE;
    pub const MPEG_STREAM_TYPE_MPEG2_PS: u32 = 0;
    pub const MPEG_STREAM_TYPE_MPEG2_TS: u32 = 1;
    pub const MPEG_STREAM_TYPE_MPEG1_SS: u32 = 2;
    pub const MPEG_STREAM_TYPE_MPEG2_DVD: u32 = 3;
    pub const MPEG_STREAM_TYPE_MPEG1_VCD: u32 = 4;
    pub const MPEG_STREAM_TYPE_MPEG2_SVCD: u32 = 5;
    pub const CID_MPEG_STREAM_PID_PMT: u32 = CID_MPEG_BASE + 1;
    pub const CID_MPEG_STREAM_PID_AUDIO: u32 = CID_MPEG_BASE + 2;
    pub const CID_MPEG_STREAM_PID_VIDEO: u32 = CID_MPEG_BASE + 3;
    pub const CID_MPEG_STREAM_PID_PCR: u32 = CID_MPEG_BASE + 4;
    pub const CID_MPEG_STREAM_PES_ID_AUDIO: u32 = CID_MPEG_BASE + 5;
    pub const CID_MPEG_STREAM_PES_ID_VIDEO: u32 = CID_MPEG_BASE + 6;
    pub const CID_MPEG_STREAM_VBI_FMT: u32 = CID_MPEG_BASE + 7;
    pub const MPEG_STREAM_VBI_FMT_NONE: u32 = 0;
    pub const MPEG_STREAM_VBI_FMT_IVTV: u32 = 1;
    pub const CID_MPEG_AUDIO_SAMPLING_FREQ: u32 = CID_MPEG_BASE + 100;
    pub const MPEG_AUDIO_SAMPLING_FREQ_44100: u32 = 0;
    pub const MPEG_AUDIO_SAMPLING_FREQ_48000: u32 = 1;
    pub const MPEG_AUDIO_SAMPLING_FREQ_32000: u32 = 2;
    pub const CID_MPEG_AUDIO_ENCODING: u32 = CID_MPEG_BASE + 101;
    pub const MPEG_AUDIO_ENCODING_LAYER_1: u32 = 0;
    pub const MPEG_AUDIO_ENCODING_LAYER_2: u32 = 1;
    pub const MPEG_AUDIO_ENCODING_LAYER_3: u32 = 2;
    pub const MPEG_AUDIO_ENCODING_AAC: u32 = 3;
    pub const MPEG_AUDIO_ENCODING_AC3: u32 = 4;
    pub const CID_MPEG_AUDIO_L1_BITRATE: u32 = CID_MPEG_BASE + 102;
    pub const MPEG_AUDIO_L1_BITRATE_32K: u32 = 0;
    pub const MPEG_AUDIO_L1_BITRATE_64K: u32 = 1;
    pub const MPEG_AUDIO_L1_BITRATE_96K: u32 = 2;
    pub const MPEG_AUDIO_L1_BITRATE_128K: u32 = 3;
    pub const MPEG_AUDIO_L1_BITRATE_160K: u32 = 4;
    pub const MPEG_AUDIO_L1_BITRATE_192K: u32 = 5;
    pub const MPEG_AUDIO_L1_BITRATE_224K: u32 = 6;
    pub const MPEG_AUDIO_L1_BITRATE_256K: u32 = 7;
    pub const MPEG_AUDIO_L1_BITRATE_288K: u32 = 8;
    pub const MPEG_AUDIO_L1_BITRATE_320K: u32 = 9;
    pub const MPEG_AUDIO_L1_BITRATE_352K: u32 = 10;
    pub const MPEG_AUDIO_L1_BITRATE_384K: u32 = 11;
    pub const MPEG_AUDIO_L1_BITRATE_416K: u32 = 12;
    pub const MPEG_AUDIO_L1_BITRATE_448K: u32 = 13;
    pub const CID_MPEG_AUDIO_L2_BITRATE: u32 = CID_MPEG_BASE + 103;
    pub const MPEG_AUDIO_L2_BITRATE_32K: u32 = 0;
    pub const MPEG_AUDIO_L2_BITRATE_48K: u32 = 1;
    pub const MPEG_AUDIO_L2_BITRATE_56K: u32 = 2;
    pub const MPEG_AUDIO_L2_BITRATE_64K: u32 = 3;
    pub const MPEG_AUDIO_L2_BITRATE_80K: u32 = 4;
    pub const MPEG_AUDIO_L2_BITRATE_96K: u32 = 5;
    pub const MPEG_AUDIO_L2_BITRATE_112K: u32 = 6;
    pub const MPEG_AUDIO_L2_BITRATE_128K: u32 = 7;
    pub const MPEG_AUDIO_L2_BITRATE_160K: u32 = 8;
    pub const MPEG_AUDIO_L2_BITRATE_192K: u32 = 9;
    pub const MPEG_AUDIO_L2_BITRATE_224K: u32 = 10;
    pub const MPEG_AUDIO_L2_BITRATE_256K: u32 = 11;
    pub const MPEG_AUDIO_L2_BITRATE_320K: u32 = 12;
    pub const MPEG_AUDIO_L2_BITRATE_384K: u32 = 13;
    pub const CID_MPEG_AUDIO_L3_BITRATE: u32 = CID_MPEG_BASE + 104;
    pub const MPEG_AUDIO_L3_BITRATE_32K: u32 = 0;
    pub const MPEG_AUDIO_L3_BITRATE_40K: u32 = 1;
    pub const MPEG_AUDIO_L3_BITRATE_48K: u32 = 2;
    pub const MPEG_AUDIO_L3_BITRATE_56K: u32 = 3;
    pub const MPEG_AUDIO_L3_BITRATE_64K: u32 = 4;
    pub const MPEG_AUDIO_L3_BITRATE_80K: u32 = 5;
    pub const MPEG_AUDIO_L3_BITRATE_96K: u32 = 6;
    pub const MPEG_AUDIO_L3_BITRATE_112K: u32 = 7;
    pub const MPEG_AUDIO_L3_BITRATE_128K: u32 = 8;
    pub const MPEG_AUDIO_L3_BITRATE_160K: u32 = 9;
    pub const MPEG_AUDIO_L3_BITRATE_192K: u32 = 10;
    pub const MPEG_AUDIO_L3_BITRATE_224K: u32 = 11;
    pub const MPEG_AUDIO_L3_BITRATE_256K: u32 = 12;
    pub const MPEG_AUDIO_L3_BITRATE_320K: u32 = 13;
    pub const CID_MPEG_AUDIO_MODE: u32 = CID_MPEG_BASE + 105;
    pub const MPEG_AUDIO_MODE_STEREO: u32 = 0;
    pub const MPEG_AUDIO_MODE_JOINT_STEREO: u32 = 1;
    pub const MPEG_AUDIO_MODE_DUAL: u32 = 2;
    pub const MPEG_AUDIO_MODE_MONO: u32 = 3;
    pub const CID_MPEG_AUDIO_MODE_EXTENSION: u32 = CID_MPEG_BASE + 106;
    pub const MPEG_AUDIO_MODE_EXTENSION_BOUND_4: u32 = 0;
    pub const MPEG_AUDIO_MODE_EXTENSION_BOUND_8: u32 = 1;
    pub const MPEG_AUDIO_MODE_EXTENSION_BOUND_12: u32 = 2;
    pub const MPEG_AUDIO_MODE_EXTENSION_BOUND_16: u32 = 3;
    pub const CID_MPEG_AUDIO_EMPHASIS: u32 = CID_MPEG_BASE + 107;
    pub const MPEG_AUDIO_EMPHASIS_NONE: u32 = 0;
    pub const MPEG_AUDIO_EMPHASIS_50_DIV_15_uS: u32 = 1;
    pub const MPEG_AUDIO_EMPHASIS_CCITT_J17: u32 = 2;
    pub const CID_MPEG_AUDIO_CRC: u32 = CID_MPEG_BASE + 108;
    pub const MPEG_AUDIO_CRC_NONE: u32 = 0;
    pub const MPEG_AUDIO_CRC_CRC16: u32 = 1;
    pub const CID_MPEG_AUDIO_MUTE: u32 = CID_MPEG_BASE + 109;
    pub const CID_MPEG_AUDIO_AAC_BITRATE: u32 = CID_MPEG_BASE + 110;
    pub const CID_MPEG_AUDIO_AC3_BITRATE: u32 = CID_MPEG_BASE + 111;
    pub const MPEG_AUDIO_AC3_BITRATE_32K: u32 = 0;
    pub const MPEG_AUDIO_AC3_BITRATE_40K: u32 = 1;
    pub const MPEG_AUDIO_AC3_BITRATE_48K: u32 = 2;
    pub const MPEG_AUDIO_AC3_BITRATE_56K: u32 = 3;
    pub const MPEG_AUDIO_AC3_BITRATE_64K: u32 = 4;
    pub const MPEG_AUDIO_AC3_BITRATE_80K: u32 = 5;
    pub const MPEG_AUDIO_AC3_BITRATE_96K: u32 = 6;
    pub const MPEG_AUDIO_AC3_BITRATE_112K: u32 = 7;
    pub const MPEG_AUDIO_AC3_BITRATE_128K: u32 = 8;
    pub const MPEG_AUDIO_AC3_BITRATE_160K: u32 = 9;
    pub const MPEG_AUDIO_AC3_BITRATE_192K: u32 = 10;
    pub const MPEG_AUDIO_AC3_BITRATE_224K: u32 = 11;
    pub const MPEG_AUDIO_AC3_BITRATE_256K: u32 = 12;
    pub const MPEG_AUDIO_AC3_BITRATE_320K: u32 = 13;
    pub const MPEG_AUDIO_AC3_BITRATE_384K: u32 = 14;
    pub const MPEG_AUDIO_AC3_BITRATE_448K: u32 = 15;
    pub const MPEG_AUDIO_AC3_BITRATE_512K: u32 = 16;
    pub const MPEG_AUDIO_AC3_BITRATE_576K: u32 = 17;
    pub const MPEG_AUDIO_AC3_BITRATE_640K: u32 = 18;
    pub const CID_MPEG_AUDIO_DEC_PLAYBACK: u32 = CID_MPEG_BASE + 112;
    pub const MPEG_AUDIO_DEC_PLAYBACK_AUTO: u32 = 0;
    pub const MPEG_AUDIO_DEC_PLAYBACK_STEREO: u32 = 1;
    pub const MPEG_AUDIO_DEC_PLAYBACK_LEFT: u32 = 2;
    pub const MPEG_AUDIO_DEC_PLAYBACK_RIGHT: u32 = 3;
    pub const MPEG_AUDIO_DEC_PLAYBACK_MONO: u32 = 4;
    pub const MPEG_AUDIO_DEC_PLAYBACK_SWAPPED_STEREO: u32 = 5;
    pub const CID_MPEG_AUDIO_DEC_MULTILINGUAL_PLAYBACK: u32 = CID_MPEG_BASE + 113;
    pub const CID_MPEG_VIDEO_ENCODING: u32 = CID_MPEG_BASE + 200;
    pub const MPEG_VIDEO_ENCODING_MPEG_1: u32 = 0;
    pub const MPEG_VIDEO_ENCODING_MPEG_2: u32 = 1;
    pub const MPEG_VIDEO_ENCODING_MPEG_4_AVC: u32 = 2;
    pub const CID_MPEG_VIDEO_ASPECT: u32 = CID_MPEG_BASE + 201;
    pub const MPEG_VIDEO_ASPECT_1x1: u32 = 0;
    pub const MPEG_VIDEO_ASPECT_4x3: u32 = 1;
    pub const MPEG_VIDEO_ASPECT_16x9: u32 = 2;
    pub const MPEG_VIDEO_ASPECT_221x100: u32 = 3;
    pub const CID_MPEG_VIDEO_B_FRAMES: u32 = CID_MPEG_BASE + 202;
    pub const CID_MPEG_VIDEO_GOP_SIZE: u32 = CID_MPEG_BASE + 203;
    pub const CID_MPEG_VIDEO_GOP_CLOSURE: u32 = CID_MPEG_BASE + 204;
    pub const CID_MPEG_VIDEO_PULLDOWN: u32 = CID_MPEG_BASE + 205;
    pub const CID_MPEG_VIDEO_BITRATE_MODE: u32 = CID_MPEG_BASE + 206;
    pub const MPEG_VIDEO_BITRATE_MODE_VBR: u32 = 0;
    pub const MPEG_VIDEO_BITRATE_MODE_CBR: u32 = 1;
    pub const CID_MPEG_VIDEO_BITRATE: u32 = CID_MPEG_BASE + 207;
    pub const CID_MPEG_VIDEO_BITRATE_PEAK: u32 = CID_MPEG_BASE + 208;
    pub const CID_MPEG_VIDEO_TEMPORAL_DECIMATION: u32 = CID_MPEG_BASE + 209;
    pub const CID_MPEG_VIDEO_MUTE: u32 = CID_MPEG_BASE + 210;
    pub const CID_MPEG_VIDEO_MUTE_YUV: u32 = CID_MPEG_BASE + 211;
    pub const CID_MPEG_VIDEO_DECODER_SLICE_INTERFACE: u32 = CID_MPEG_BASE + 212;
    pub const CID_MPEG_VIDEO_DECODER_MPEG4_DEBLOCK_FILTER: u32 = CID_MPEG_BASE + 213;
    pub const CID_MPEG_VIDEO_CYCLIC_INTRA_REFRESH_MB: u32 = CID_MPEG_BASE + 214;
    pub const CID_MPEG_VIDEO_FRAME_RC_ENABLE: u32 = CID_MPEG_BASE + 215;
    pub const CID_MPEG_VIDEO_HEADER_MODE: u32 = CID_MPEG_BASE + 216;
    pub const MPEG_VIDEO_HEADER_MODE_SEPARATE: u32 = 0;
    pub const MPEG_VIDEO_HEADER_MODE_JOINED_WITH_1ST_FRAME: u32 = 1;
    pub const CID_MPEG_VIDEO_MAX_REF_PIC: u32 = CID_MPEG_BASE + 217;
    pub const CID_MPEG_VIDEO_MB_RC_ENABLE: u32 = CID_MPEG_BASE + 218;
    pub const CID_MPEG_VIDEO_MULTI_SLICE_MAX_BYTES: u32 = CID_MPEG_BASE + 219;
    pub const CID_MPEG_VIDEO_MULTI_SLICE_MAX_MB: u32 = CID_MPEG_BASE + 220;
    pub const CID_MPEG_VIDEO_MULTI_SLICE_MODE: u32 = CID_MPEG_BASE + 221;
    pub const MPEG_VIDEO_MULTI_SLICE_MODE_SINGLE: u32 = 0;
    pub const MPEG_VIDEO_MULTI_SICE_MODE_MAX_MB: u32 = 1;
    pub const MPEG_VIDEO_MULTI_SICE_MODE_MAX_BYTES: u32 = 2;
    pub const CID_MPEG_VIDEO_VBV_SIZE: u32 = CID_MPEG_BASE + 222;
    pub const CID_MPEG_VIDEO_DEC_PTS: u32 = CID_MPEG_BASE + 223;
    pub const CID_MPEG_VIDEO_DEC_FRAME: u32 = CID_MPEG_BASE + 224;
    pub const CID_MPEG_VIDEO_VBV_DELAY: u32 = CID_MPEG_BASE + 225;
    pub const CID_MPEG_VIDEO_REPEAT_SEQ_HEADER: u32 = CID_MPEG_BASE + 226;
    pub const CID_MPEG_VIDEO_MV_H_SEARCH_RANGE: u32 = CID_MPEG_BASE + 227;
    pub const CID_MPEG_VIDEO_MV_V_SEARCH_RANGE: u32 = CID_MPEG_BASE + 228;
    pub const CID_MPEG_VIDEO_H263_I_FRAME_QP: u32 = CID_MPEG_BASE + 300;
    pub const CID_MPEG_VIDEO_H263_P_FRAME_QP: u32 = CID_MPEG_BASE + 301;
    pub const CID_MPEG_VIDEO_H263_B_FRAME_QP: u32 = CID_MPEG_BASE + 302;
    pub const CID_MPEG_VIDEO_H263_MIN_QP: u32 = CID_MPEG_BASE + 303;
    pub const CID_MPEG_VIDEO_H263_MAX_QP: u32 = CID_MPEG_BASE + 304;
    pub const CID_MPEG_VIDEO_H264_I_FRAME_QP: u32 = CID_MPEG_BASE + 350;
    pub const CID_MPEG_VIDEO_H264_P_FRAME_QP: u32 = CID_MPEG_BASE + 351;
    pub const CID_MPEG_VIDEO_H264_B_FRAME_QP: u32 = CID_MPEG_BASE + 352;
    pub const CID_MPEG_VIDEO_H264_MIN_QP: u32 = CID_MPEG_BASE + 353;
    pub const CID_MPEG_VIDEO_H264_MAX_QP: u32 = CID_MPEG_BASE + 354;
    pub const CID_MPEG_VIDEO_H264_8X8_TRANSFORM: u32 = CID_MPEG_BASE + 355;
    pub const CID_MPEG_VIDEO_H264_CPB_SIZE: u32 = CID_MPEG_BASE + 356;
    pub const CID_MPEG_VIDEO_H264_ENTROPY_MODE: u32 = CID_MPEG_BASE + 357;
    pub const MPEG_VIDEO_H264_ENTROPY_MODE_CAVLC: u32 = 0;
    pub const MPEG_VIDEO_H264_ENTROPY_MODE_CABAC: u32 = 1;
    pub const CID_MPEG_VIDEO_H264_I_PERIOD: u32 = CID_MPEG_BASE + 358;
    pub const CID_MPEG_VIDEO_H264_LEVEL: u32 = CID_MPEG_BASE + 359;
    pub const MPEG_VIDEO_H264_LEVEL_1_0: u32 = 0;
    pub const MPEG_VIDEO_H264_LEVEL_1B: u32 = 1;
    pub const MPEG_VIDEO_H264_LEVEL_1_1: u32 = 2;
    pub const MPEG_VIDEO_H264_LEVEL_1_2: u32 = 3;
    pub const MPEG_VIDEO_H264_LEVEL_1_3: u32 = 4;
    pub const MPEG_VIDEO_H264_LEVEL_2_0: u32 = 5;
    pub const MPEG_VIDEO_H264_LEVEL_2_1: u32 = 6;
    pub const MPEG_VIDEO_H264_LEVEL_2_2: u32 = 7;
    pub const MPEG_VIDEO_H264_LEVEL_3_0: u32 = 8;
    pub const MPEG_VIDEO_H264_LEVEL_3_1: u32 = 9;
    pub const MPEG_VIDEO_H264_LEVEL_3_2: u32 = 10;
    pub const MPEG_VIDEO_H264_LEVEL_4_0: u32 = 11;
    pub const MPEG_VIDEO_H264_LEVEL_4_1: u32 = 12;
    pub const MPEG_VIDEO_H264_LEVEL_4_2: u32 = 13;
    pub const MPEG_VIDEO_H264_LEVEL_5_0: u32 = 14;
    pub const MPEG_VIDEO_H264_LEVEL_5_1: u32 = 15;
    pub const CID_MPEG_VIDEO_H264_LOOP_FILTER_ALPHA: u32 = CID_MPEG_BASE + 360;
    pub const CID_MPEG_VIDEO_H264_LOOP_FILTER_BETA: u32 = CID_MPEG_BASE + 361;
    pub const CID_MPEG_VIDEO_H264_LOOP_FILTER_MODE: u32 = CID_MPEG_BASE + 362;
    pub const MPEG_VIDEO_H264_LOOP_FILTER_MODE_ENABLED: u32 = 0;
    pub const MPEG_VIDEO_H264_LOOP_FILTER_MODE_DISABLED: u32 = 1;
    pub const MPEG_VIDEO_H264_LOOP_FILTER_MODE_DISABLED_AT_SLICE_BOUNDARY: u32 = 2;
    pub const CID_MPEG_VIDEO_H264_PROFILE: u32 = CID_MPEG_BASE + 363;
    pub const MPEG_VIDEO_H264_PROFILE_BASELINE: u32 = 0;
    pub const MPEG_VIDEO_H264_PROFILE_CONSTRAINED_BASELINE: u32 = 1;
    pub const MPEG_VIDEO_H264_PROFILE_MAIN: u32 = 2;
    pub const MPEG_VIDEO_H264_PROFILE_EXTENDED: u32 = 3;
    pub const MPEG_VIDEO_H264_PROFILE_HIGH: u32 = 4;
    pub const MPEG_VIDEO_H264_PROFILE_HIGH_10: u32 = 5;
    pub const MPEG_VIDEO_H264_PROFILE_HIGH_422: u32 = 6;
    pub const MPEG_VIDEO_H264_PROFILE_HIGH_444_PREDICTIVE: u32 = 7;
    pub const MPEG_VIDEO_H264_PROFILE_HIGH_10_INTRA: u32 = 8;
    pub const MPEG_VIDEO_H264_PROFILE_HIGH_422_INTRA: u32 = 9;
    pub const MPEG_VIDEO_H264_PROFILE_HIGH_444_INTRA: u32 = 10;
    pub const MPEG_VIDEO_H264_PROFILE_CAVLC_444_INTRA: u32 = 11;
    pub const MPEG_VIDEO_H264_PROFILE_SCALABLE_BASELINE: u32 = 12;
    pub const MPEG_VIDEO_H264_PROFILE_SCALABLE_HIGH: u32 = 13;
    pub const MPEG_VIDEO_H264_PROFILE_SCALABLE_HIGH_INTRA: u32 = 14;
    pub const MPEG_VIDEO_H264_PROFILE_STEREO_HIGH: u32 = 15;
    pub const MPEG_VIDEO_H264_PROFILE_MULTIVIEW_HIGH: u32 = 16;
    pub const CID_MPEG_VIDEO_H264_VUI_EXT_SAR_HEIGHT: u32 = CID_MPEG_BASE + 364;
    pub const CID_MPEG_VIDEO_H264_VUI_EXT_SAR_WIDTH: u32 = CID_MPEG_BASE + 365;
    pub const CID_MPEG_VIDEO_H264_VUI_SAR_ENABLE: u32 = CID_MPEG_BASE + 366;
    pub const CID_MPEG_VIDEO_H264_VUI_SAR_IDC: u32 = CID_MPEG_BASE + 367;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_UNSPECIFIED: u32 = 0;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_1x1: u32 = 1;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_12x11: u32 = 2;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_10x11: u32 = 3;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_16x11: u32 = 4;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_40x33: u32 = 5;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_24x11: u32 = 6;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_20x11: u32 = 7;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_32x11: u32 = 8;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_80x33: u32 = 9;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_18x11: u32 = 10;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_15x11: u32 = 11;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_64x33: u32 = 12;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_160x99: u32 = 13;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_4x3: u32 = 14;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_3x2: u32 = 15;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_2x1: u32 = 16;
    pub const MPEG_VIDEO_H264_VUI_SAR_IDC_EXTENDED: u32 = 17;
    pub const CID_MPEG_VIDEO_H264_SEI_FRAME_PACKING: u32 = CID_MPEG_BASE + 368;
    pub const CID_MPEG_VIDEO_H264_SEI_FP_CURRENT_FRAME_0: u32 = CID_MPEG_BASE + 369;
    pub const CID_MPEG_VIDEO_H264_SEI_FP_ARRANGEMENT_TYPE: u32 = CID_MPEG_BASE + 370;
    pub const MPEG_VIDEO_H264_SEI_FP_ARRANGEMENT_TYPE_CHECKERBOARD: u32 = 0;
    pub const MPEG_VIDEO_H264_SEI_FP_ARRANGEMENT_TYPE_COLUMN: u32 = 1;
    pub const MPEG_VIDEO_H264_SEI_FP_ARRANGEMENT_TYPE_ROW: u32 = 2;
    pub const MPEG_VIDEO_H264_SEI_FP_ARRANGEMENT_TYPE_SIDE_BY_SIDE: u32 = 3;
    pub const MPEG_VIDEO_H264_SEI_FP_ARRANGEMENT_TYPE_TOP_BOTTOM: u32 = 4;
    pub const MPEG_VIDEO_H264_SEI_FP_ARRANGEMENT_TYPE_TEMPORAL: u32 = 5;
    pub const CID_MPEG_VIDEO_H264_FMO: u32 = CID_MPEG_BASE + 371;
    pub const CID_MPEG_VIDEO_H264_FMO_MAP_TYPE: u32 = CID_MPEG_BASE + 372;
    pub const MPEG_VIDEO_H264_FMO_MAP_TYPE_INTERLEAVED_SLICES: u32 = 0;
    pub const MPEG_VIDEO_H264_FMO_MAP_TYPE_SCATTERED_SLICES: u32 = 1;
    pub const MPEG_VIDEO_H264_FMO_MAP_TYPE_FOREGROUND_WITH_LEFT_OVER: u32 = 2;
    pub const MPEG_VIDEO_H264_FMO_MAP_TYPE_BOX_OUT: u32 = 3;
    pub const MPEG_VIDEO_H264_FMO_MAP_TYPE_RASTER_SCAN: u32 = 4;
    pub const MPEG_VIDEO_H264_FMO_MAP_TYPE_WIPE_SCAN: u32 = 5;
    pub const MPEG_VIDEO_H264_FMO_MAP_TYPE_EXPLICIT: u32 = 6;
    pub const CID_MPEG_VIDEO_H264_FMO_SLICE_GROUP: u32 = CID_MPEG_BASE + 373;
    pub const CID_MPEG_VIDEO_H264_FMO_CHANGE_DIRECTION: u32 = CID_MPEG_BASE + 374;
    pub const MPEG_VIDEO_H264_FMO_CHANGE_DIR_RIGHT: u32 = 0;
    pub const MPEG_VIDEO_H264_FMO_CHANGE_DIR_LEFT: u32 = 1;
    pub const CID_MPEG_VIDEO_H264_FMO_CHANGE_RATE: u32 = CID_MPEG_BASE + 375;
    pub const CID_MPEG_VIDEO_H264_FMO_RUN_LENGTH: u32 = CID_MPEG_BASE + 376;
    pub const CID_MPEG_VIDEO_H264_ASO: u32 = CID_MPEG_BASE + 377;
    pub const CID_MPEG_VIDEO_H264_ASO_SLICE_ORDER: u32 = CID_MPEG_BASE + 378;
    pub const CID_MPEG_VIDEO_H264_HIERARCHICAL_CODING: u32 = CID_MPEG_BASE + 379;
    pub const CID_MPEG_VIDEO_H264_HIERARCHICAL_CODING_TYPE: u32 = CID_MPEG_BASE + 380;
    pub const MPEG_VIDEO_H264_HIERARCHICAL_CODING_B: u32 = 0;
    pub const MPEG_VIDEO_H264_HIERARCHICAL_CODING_P: u32 = 1;
    pub const CID_MPEG_VIDEO_H264_HIERARCHICAL_CODING_LAYER: u32 = CID_MPEG_BASE + 381;
    pub const CID_MPEG_VIDEO_H264_HIERARCHICAL_CODING_LAYER_QP: u32 = CID_MPEG_BASE + 382;
    pub const CID_MPEG_VIDEO_MPEG4_I_FRAME_QP: u32 = CID_MPEG_BASE + 400;
    pub const CID_MPEG_VIDEO_MPEG4_P_FRAME_QP: u32 = CID_MPEG_BASE + 401;
    pub const CID_MPEG_VIDEO_MPEG4_B_FRAME_QP: u32 = CID_MPEG_BASE + 402;
    pub const CID_MPEG_VIDEO_MPEG4_MIN_QP: u32 = CID_MPEG_BASE + 403;
    pub const CID_MPEG_VIDEO_MPEG4_MAX_QP: u32 = CID_MPEG_BASE + 404;
    pub const CID_MPEG_VIDEO_MPEG4_LEVEL: u32 = CID_MPEG_BASE + 405;
    pub const MPEG_VIDEO_MPEG4_LEVEL_0: u32 = 0;
    pub const MPEG_VIDEO_MPEG4_LEVEL_0B: u32 = 1;
    pub const MPEG_VIDEO_MPEG4_LEVEL_1: u32 = 2;
    pub const MPEG_VIDEO_MPEG4_LEVEL_2: u32 = 3;
    pub const MPEG_VIDEO_MPEG4_LEVEL_3: u32 = 4;
    pub const MPEG_VIDEO_MPEG4_LEVEL_3B: u32 = 5;
    pub const MPEG_VIDEO_MPEG4_LEVEL_4: u32 = 6;
    pub const MPEG_VIDEO_MPEG4_LEVEL_5: u32 = 7;
    pub const CID_MPEG_VIDEO_MPEG4_PROFILE: u32 = CID_MPEG_BASE + 406;
    pub const MPEG_VIDEO_MPEG4_PROFILE_SIMPLE: u32 = 0;
    pub const MPEG_VIDEO_MPEG4_PROFILE_ADVANCED_SIMPLE: u32 = 1;
    pub const MPEG_VIDEO_MPEG4_PROFILE_CORE: u32 = 2;
    pub const MPEG_VIDEO_MPEG4_PROFILE_SIMPLE_SCALABLE: u32 = 3;
    pub const MPEG_VIDEO_MPEG4_PROFILE_ADVANCED_CODING_EFFICIENCY: u32 = 4;
    pub const CID_MPEG_VIDEO_MPEG4_QPEL: u32 = CID_MPEG_BASE + 407;
    pub const CID_MPEG_VIDEO_VPX_NUM_PARTITIONS: u32 = CID_MPEG_BASE + 500;
    pub const CID_MPEG_VIDEO_VPX_1_PARTITION: u32 = 0;
    pub const CID_MPEG_VIDEO_VPX_2_PARTITIONS: u32 = 1;
    pub const CID_MPEG_VIDEO_VPX_4_PARTITIONS: u32 = 2;
    pub const CID_MPEG_VIDEO_VPX_8_PARTITIONS: u32 = 3;
    pub const CID_MPEG_VIDEO_VPX_IMD_DISABLE_4X4: u32 = CID_MPEG_BASE + 501;
    pub const CID_MPEG_VIDEO_VPX_NUM_REF_FRAMES: u32 = CID_MPEG_BASE + 502;
    pub const CID_MPEG_VIDEO_VPX_1_REF_FRAME: u32 = 0;
    pub const CID_MPEG_VIDEO_VPX_2_REF_FRAME: u32 = 1;
    pub const CID_MPEG_VIDEO_VPX_3_REF_FRAME: u32 = 2;
    pub const CID_MPEG_VIDEO_VPX_FILTER_LEVEL: u32 = CID_MPEG_BASE + 503;
    pub const CID_MPEG_VIDEO_VPX_FILTER_SHARPNESS: u32 = CID_MPEG_BASE + 504;
    pub const CID_MPEG_VIDEO_VPX_GOLDEN_FRAME_REF_PERIOD: u32 = CID_MPEG_BASE + 505;
    pub const CID_MPEG_VIDEO_VPX_GOLDEN_FRAME_SEL: u32 = CID_MPEG_BASE + 506;
    pub const CID_MPEG_VIDEO_VPX_GOLDEN_FRAME_USE_PREV: u32 = 0;
    pub const CID_MPEG_VIDEO_VPX_GOLDEN_FRAME_USE_REF_PERIOD: u32 = 1;
    pub const CID_MPEG_VIDEO_VPX_MIN_QP: u32 = CID_MPEG_BASE + 507;
    pub const CID_MPEG_VIDEO_VPX_MAX_QP: u32 = CID_MPEG_BASE + 508;
    pub const CID_MPEG_VIDEO_VPX_I_FRAME_QP: u32 = CID_MPEG_BASE + 509;
    pub const CID_MPEG_VIDEO_VPX_P_FRAME_QP: u32 = CID_MPEG_BASE + 510;
    pub const CID_MPEG_VIDEO_VPX_PROFILE: u32 = CID_MPEG_BASE + 511;
    pub const CID_MPEG_CX2341X_BASE: u32 = CLASS_MPEG | 0x1000;
    pub const CID_MPEG_CX2341X_VIDEO_SPATIAL_FILTER_MODE: u32 = CID_MPEG_CX2341X_BASE;
    pub const MPEG_CX2341X_VIDEO_SPATIAL_FILTER_MODE_MANUAL: u32 = 0;
    pub const MPEG_CX2341X_VIDEO_SPATIAL_FILTER_MODE_AUTO: u32 = 1;
    pub const CID_MPEG_CX2341X_VIDEO_SPATIAL_FILTER: u32 = CID_MPEG_CX2341X_BASE + 1;
    pub const CID_MPEG_CX2341X_VIDEO_LUMA_SPATIAL_FILTER_TYPE: u32 = CID_MPEG_CX2341X_BASE + 2;
    pub const MPEG_CX2341X_VIDEO_LUMA_SPATIAL_FILTER_TYPE_OFF: u32 = 0;
    pub const MPEG_CX2341X_VIDEO_LUMA_SPATIAL_FILTER_TYPE_1D_HOR: u32 = 1;
    pub const MPEG_CX2341X_VIDEO_LUMA_SPATIAL_FILTER_TYPE_1D_VERT: u32 = 2;
    pub const MPEG_CX2341X_VIDEO_LUMA_SPATIAL_FILTER_TYPE_2D_HV_SEPARABLE: u32 = 3;
    pub const MPEG_CX2341X_VIDEO_LUMA_SPATIAL_FILTER_TYPE_2D_SYM_NON_SEPARABLE: u32 = 4;
    pub const CID_MPEG_CX2341X_VIDEO_CHROMA_SPATIAL_FILTER_TYPE: u32 = CID_MPEG_CX2341X_BASE + 3;
    pub const MPEG_CX2341X_VIDEO_CHROMA_SPATIAL_FILTER_TYPE_OFF: u32 = 0;
    pub const MPEG_CX2341X_VIDEO_CHROMA_SPATIAL_FILTER_TYPE_1D_HOR: u32 = 1;
    pub const CID_MPEG_CX2341X_VIDEO_TEMPORAL_FILTER_MODE: u32 = CID_MPEG_CX2341X_BASE + 4;
    pub const MPEG_CX2341X_VIDEO_TEMPORAL_FILTER_MODE_MANUAL: u32 = 0;
    pub const MPEG_CX2341X_VIDEO_TEMPORAL_FILTER_MODE_AUTO: u32 = 1;
    pub const CID_MPEG_CX2341X_VIDEO_TEMPORAL_FILTER: u32 = CID_MPEG_CX2341X_BASE + 5;
    pub const CID_MPEG_CX2341X_VIDEO_MEDIAN_FILTER_TYPE: u32 = CID_MPEG_CX2341X_BASE + 6;
    pub const MPEG_CX2341X_VIDEO_MEDIAN_FILTER_TYPE_OFF: u32 = 0;
    pub const MPEG_CX2341X_VIDEO_MEDIAN_FILTER_TYPE_HOR: u32 = 1;
    pub const MPEG_CX2341X_VIDEO_MEDIAN_FILTER_TYPE_VERT: u32 = 2;
    pub const MPEG_CX2341X_VIDEO_MEDIAN_FILTER_TYPE_HOR_VERT: u32 = 3;
    pub const MPEG_CX2341X_VIDEO_MEDIAN_FILTER_TYPE_DIAG: u32 = 4;
    pub const CID_MPEG_CX2341X_VIDEO_LUMA_MEDIAN_FILTER_BOTTOM: u32 = CID_MPEG_CX2341X_BASE + 7;
    pub const CID_MPEG_CX2341X_VIDEO_LUMA_MEDIAN_FILTER_TOP: u32 = CID_MPEG_CX2341X_BASE + 8;
    pub const CID_MPEG_CX2341X_VIDEO_CHROMA_MEDIAN_FILTER_BOTTOM: u32 = CID_MPEG_CX2341X_BASE + 9;
    pub const CID_MPEG_CX2341X_VIDEO_CHROMA_MEDIAN_FILTER_TOP: u32 = CID_MPEG_CX2341X_BASE + 10;
    pub const CID_MPEG_CX2341X_STREAM_INSERT_NAV_PACKETS: u32 = CID_MPEG_CX2341X_BASE + 11;
    pub const CID_MPEG_MFC51_BASE: u32 = CLASS_MPEG | 0x1100;
    pub const CID_MPEG_MFC51_VIDEO_DECODER_H264_DISPLAY_DELAY: u32 = CID_MPEG_MFC51_BASE;
    pub const CID_MPEG_MFC51_VIDEO_DECODER_H264_DISPLAY_DELAY_ENABLE: u32 = CID_MPEG_MFC51_BASE + 1;
    pub const CID_MPEG_MFC51_VIDEO_FRAME_SKIP_MODE: u32 = CID_MPEG_MFC51_BASE + 2;
    pub const MPEG_MFC51_VIDEO_FRAME_SKIP_MODE_DISABLED: u32 = 0;
    pub const MPEG_MFC51_VIDEO_FRAME_SKIP_MODE_LEVEL_LIMIT: u32 = 1;
    pub const MPEG_MFC51_VIDEO_FRAME_SKIP_MODE_BUF_LIMIT: u32 = 2;
    pub const CID_MPEG_MFC51_VIDEO_FORCE_FRAME_TYPE: u32 = CID_MPEG_MFC51_BASE + 3;
    pub const MPEG_MFC51_VIDEO_FORCE_FRAME_TYPE_DISABLED: u32 = 0;
    pub const MPEG_MFC51_VIDEO_FORCE_FRAME_TYPE_I_FRAME: u32 = 1;
    pub const MPEG_MFC51_VIDEO_FORCE_FRAME_TYPE_NOT_CODED: u32 = 2;
    pub const CID_MPEG_MFC51_VIDEO_PADDING: u32 = CID_MPEG_MFC51_BASE + 4;
    pub const CID_MPEG_MFC51_VIDEO_PADDING_YUV: u32 = CID_MPEG_MFC51_BASE + 5;
    pub const CID_MPEG_MFC51_VIDEO_RC_FIXED_TARGET_BIT: u32 = CID_MPEG_MFC51_BASE + 6;
    pub const CID_MPEG_MFC51_VIDEO_RC_REACTION_COEFF: u32 = CID_MPEG_MFC51_BASE + 7;
    pub const CID_MPEG_MFC51_VIDEO_H264_ADAPTIVE_RC_ACTIVITY: u32 = CID_MPEG_MFC51_BASE + 50;
    pub const CID_MPEG_MFC51_VIDEO_H264_ADAPTIVE_RC_DARK: u32 = CID_MPEG_MFC51_BASE + 51;
    pub const CID_MPEG_MFC51_VIDEO_H264_ADAPTIVE_RC_SMOOTH: u32 = CID_MPEG_MFC51_BASE + 52;
    pub const CID_MPEG_MFC51_VIDEO_H264_ADAPTIVE_RC_STATIC: u32 = CID_MPEG_MFC51_BASE + 53;
    pub const CID_MPEG_MFC51_VIDEO_H264_NUM_REF_PIC_FOR_P: u32 = CID_MPEG_MFC51_BASE + 54;
    pub const CID_CAMERA_CLASS_BASE: u32 = CLASS_CAMERA | 0x900;
    pub const CID_CAMERA_CLASS: u32 = CLASS_CAMERA | 1;
    pub const CID_EXPOSURE_AUTO: u32 = CID_CAMERA_CLASS_BASE + 1;
    pub const EXPOSURE_AUTO: u32 = 0;
    pub const EXPOSURE_MANUAL: u32 = 1;
    pub const EXPOSURE_SHUTTER_PRIORITY: u32 = 2;
    pub const EXPOSURE_APERTURE_PRIORITY: u32 = 3;
    pub const CID_EXPOSURE_ABSOLUTE: u32 = CID_CAMERA_CLASS_BASE + 2;
    pub const CID_EXPOSURE_AUTO_PRIORITY: u32 = CID_CAMERA_CLASS_BASE + 3;
    pub const CID_PAN_RELATIVE: u32 = CID_CAMERA_CLASS_BASE + 4;
    pub const CID_TILT_RELATIVE: u32 = CID_CAMERA_CLASS_BASE + 5;
    pub const CID_PAN_RESET: u32 = CID_CAMERA_CLASS_BASE + 6;
    pub const CID_TILT_RESET: u32 = CID_CAMERA_CLASS_BASE + 7;
    pub const CID_PAN_ABSOLUTE: u32 = CID_CAMERA_CLASS_BASE + 8;
    pub const CID_TILT_ABSOLUTE: u32 = CID_CAMERA_CLASS_BASE + 9;
    pub const CID_FOCUS_ABSOLUTE: u32 = CID_CAMERA_CLASS_BASE + 10;
    pub const CID_FOCUS_RELATIVE: u32 = CID_CAMERA_CLASS_BASE + 11;
    pub const CID_FOCUS_AUTO: u32 = CID_CAMERA_CLASS_BASE + 12;
    pub const CID_ZOOM_ABSOLUTE: u32 = CID_CAMERA_CLASS_BASE + 13;
    pub const CID_ZOOM_RELATIVE: u32 = CID_CAMERA_CLASS_BASE + 14;
    pub const CID_ZOOM_CONTINUOUS: u32 = CID_CAMERA_CLASS_BASE + 15;
    pub const CID_PRIVACY: u32 = CID_CAMERA_CLASS_BASE + 16;
    pub const CID_IRIS_ABSOLUTE: u32 = CID_CAMERA_CLASS_BASE + 17;
    pub const CID_IRIS_RELATIVE: u32 = CID_CAMERA_CLASS_BASE + 18;
    pub const CID_AUTO_EXPOSURE_BIAS: u32 = CID_CAMERA_CLASS_BASE + 19;
    pub const CID_AUTO_N_PRESET_WHITE_BALANCE: u32 = CID_CAMERA_CLASS_BASE + 20;
    pub const WHITE_BALANCE_MANUAL: u32 = 0;
    pub const WHITE_BALANCE_AUTO: u32 = 1;
    pub const WHITE_BALANCE_INCANDESCENT: u32 = 2;
    pub const WHITE_BALANCE_FLUORESCENT: u32 = 3;
    pub const WHITE_BALANCE_FLUORESCENT_H: u32 = 4;
    pub const WHITE_BALANCE_HORIZON: u32 = 5;
    pub const WHITE_BALANCE_DAYLIGHT: u32 = 6;
    pub const WHITE_BALANCE_FLASH: u32 = 7;
    pub const WHITE_BALANCE_CLOUDY: u32 = 8;
    pub const WHITE_BALANCE_SHADE: u32 = 9;
    pub const WHITE_BALANCE_GREYWORLD: u32 = 10;
    pub const CID_WIDE_DYNAMIC_RANGE: u32 = CID_CAMERA_CLASS_BASE + 21;
    pub const CID_IMAGE_STABILIZATION: u32 = CID_CAMERA_CLASS_BASE + 22;
    pub const CID_ISO_SENSITIVITY: u32 = CID_CAMERA_CLASS_BASE + 23;
    pub const CID_ISO_SENSITIVITY_AUTO: u32 = CID_CAMERA_CLASS_BASE + 24;
    pub const ISO_SENSITIVITY_MANUAL: u32 = 0;
    pub const ISO_SENSITIVITY_AUTO: u32 = 1;
    pub const CID_EXPOSURE_METERING: u32 = CID_CAMERA_CLASS_BASE + 25;
    pub const EXPOSURE_METERING_AVERAGE: u32 = 0;
    pub const EXPOSURE_METERING_CENTER_WEIGHTED: u32 = 1;
    pub const EXPOSURE_METERING_SPOT: u32 = 2;
    pub const EXPOSURE_METERING_MATRIX: u32 = 3;
    pub const CID_SCENE_MODE: u32 = CID_CAMERA_CLASS_BASE + 26;
    pub const SCENE_MODE_NONE: u32 = 0;
    pub const SCENE_MODE_BACKLIGHT: u32 = 1;
    pub const SCENE_MODE_BEACH_SNOW: u32 = 2;
    pub const SCENE_MODE_CANDLE_LIGHT: u32 = 3;
    pub const SCENE_MODE_DAWN_DUSK: u32 = 4;
    pub const SCENE_MODE_FALL_COLORS: u32 = 5;
    pub const SCENE_MODE_FIREWORKS: u32 = 6;
    pub const SCENE_MODE_LANDSCAPE: u32 = 7;
    pub const SCENE_MODE_NIGHT: u32 = 8;
    pub const SCENE_MODE_PARTY_INDOOR: u32 = 9;
    pub const SCENE_MODE_PORTRAIT: u32 = 10;
    pub const SCENE_MODE_SPORTS: u32 = 11;
    pub const SCENE_MODE_SUNSET: u32 = 12;
    pub const SCENE_MODE_TEXT: u32 = 13;
    pub const CID_3A_LOCK: u32 = CID_CAMERA_CLASS_BASE + 27;
    pub const LOCK_EXPOSURE: u32 = 1;
    pub const LOCK_WHITE_BALANCE: u32 = 1 << 1;
    pub const LOCK_FOCUS: u32 = 1 << 2;
    pub const CID_AUTO_FOCUS_START: u32 = CID_CAMERA_CLASS_BASE + 28;
    pub const CID_AUTO_FOCUS_STOP: u32 = CID_CAMERA_CLASS_BASE + 29;
    pub const CID_AUTO_FOCUS_STATUS: u32 = CID_CAMERA_CLASS_BASE + 30;
    pub const AUTO_FOCUS_STATUS_IDLE: u32 = 0;
    pub const AUTO_FOCUS_STATUS_BUSY: u32 = 1;
    pub const AUTO_FOCUS_STATUS_REACHED: u32 = 1 << 1;
    pub const AUTO_FOCUS_STATUS_FAILED: u32 = 1 << 2;
    pub const CID_AUTO_FOCUS_RANGE: u32 = CID_CAMERA_CLASS_BASE + 31;
    pub const AUTO_FOCUS_RANGE_AUTO: u32 = 0;
    pub const AUTO_FOCUS_RANGE_NORMAL: u32 = 1;
    pub const AUTO_FOCUS_RANGE_MACRO: u32 = 2;
    pub const AUTO_FOCUS_RANGE_INFINITY: u32 = 3;
    pub const CID_PAN_SPEED: u32 = CID_CAMERA_CLASS_BASE + 32;
    pub const CID_TILT_SPEED: u32 = CID_CAMERA_CLASS_BASE + 33;
    pub const CID_FM_TX_CLASS_BASE: u32 = CLASS_FM_TX | 0x900;
    pub const CID_FM_TX_CLASS: u32 = CLASS_FM_TX | 1;
    pub const CID_RDS_TX_DEVIATION: u32 = CID_FM_TX_CLASS_BASE + 1;
    pub const CID_RDS_TX_PI: u32 = CID_FM_TX_CLASS_BASE + 2;
    pub const CID_RDS_TX_PTY: u32 = CID_FM_TX_CLASS_BASE + 3;
    pub const CID_RDS_TX_PS_NAME: u32 = CID_FM_TX_CLASS_BASE + 5;
    pub const CID_RDS_TX_RADIO_TEXT: u32 = CID_FM_TX_CLASS_BASE + 6;
    pub const CID_RDS_TX_MONO_STEREO: u32 = CID_FM_TX_CLASS_BASE + 7;
    pub const CID_RDS_TX_ARTIFICIAL_HEAD: u32 = CID_FM_TX_CLASS_BASE + 8;
    pub const CID_RDS_TX_COMPRESSED: u32 = CID_FM_TX_CLASS_BASE + 9;
    pub const CID_RDS_TX_DYNAMIC_PTY: u32 = CID_FM_TX_CLASS_BASE + 10;
    pub const CID_RDS_TX_TRAFFIC_ANNOUNCEMENT: u32 = CID_FM_TX_CLASS_BASE + 11;
    pub const CID_RDS_TX_TRAFFIC_PROGRAM: u32 = CID_FM_TX_CLASS_BASE + 12;
    pub const CID_RDS_TX_MUSIC_SPEECH: u32 = CID_FM_TX_CLASS_BASE + 13;
    pub const CID_RDS_TX_ALT_FREQS_ENABLE: u32 = CID_FM_TX_CLASS_BASE + 14;
    pub const CID_RDS_TX_ALT_FREQS: u32 = CID_FM_TX_CLASS_BASE + 15;
    pub const CID_AUDIO_LIMITER_ENABLED: u32 = CID_FM_TX_CLASS_BASE + 64;
    pub const CID_AUDIO_LIMITER_RELEASE_TIME: u32 = CID_FM_TX_CLASS_BASE + 65;
    pub const CID_AUDIO_LIMITER_DEVIATION: u32 = CID_FM_TX_CLASS_BASE + 66;
    pub const CID_AUDIO_COMPRESSION_ENABLED: u32 = CID_FM_TX_CLASS_BASE + 80;
    pub const CID_AUDIO_COMPRESSION_GAIN: u32 = CID_FM_TX_CLASS_BASE + 81;
    pub const CID_AUDIO_COMPRESSION_THRESHOLD: u32 = CID_FM_TX_CLASS_BASE + 82;
    pub const CID_AUDIO_COMPRESSION_ATTACK_TIME: u32 = CID_FM_TX_CLASS_BASE + 83;
    pub const CID_AUDIO_COMPRESSION_RELEASE_TIME: u32 = CID_FM_TX_CLASS_BASE + 84;
    pub const CID_PILOT_TONE_ENABLED: u32 = CID_FM_TX_CLASS_BASE + 96;
    pub const CID_PILOT_TONE_DEVIATION: u32 = CID_FM_TX_CLASS_BASE + 97;
    pub const CID_PILOT_TONE_FREQUENCY: u32 = CID_FM_TX_CLASS_BASE + 98;
    pub const CID_TUNE_PREEMPHASIS: u32 = CID_FM_TX_CLASS_BASE + 112;
    pub const PREEMPHASIS_DISABLED: u32 = 0;
    pub const PREEMPHASIS_50_uS: u32 = 1;
    pub const PREEMPHASIS_75_uS: u32 = 2;
    pub const CID_TUNE_POWER_LEVEL: u32 = CID_FM_TX_CLASS_BASE + 113;
    pub const CID_TUNE_ANTENNA_CAPACITOR: u32 = CID_FM_TX_CLASS_BASE + 114;
    pub const CID_FLASH_CLASS_BASE: u32 = CLASS_FLASH | 0x900;
    pub const CID_FLASH_CLASS: u32 = CLASS_FLASH | 1;
    pub const CID_FLASH_LED_MODE: u32 = CID_FLASH_CLASS_BASE + 1;
    pub const FLASH_LED_MODE_NONE: u32 = 0;
    pub const FLASH_LED_MODE_FLASH: u32 = 1;
    pub const FLASH_LED_MODE_TORCH: u32 = 2;
    pub const CID_FLASH_STROBE_SOURCE: u32 = CID_FLASH_CLASS_BASE + 2;
    pub const FLASH_STROBE_SOURCE_SOFTWARE: u32 = 0;
    pub const FLASH_STROBE_SOURCE_EXTERNAL: u32 = 1;
    pub const CID_FLASH_STROBE: u32 = CID_FLASH_CLASS_BASE + 3;
    pub const CID_FLASH_STROBE_STOP: u32 = CID_FLASH_CLASS_BASE + 4;
    pub const CID_FLASH_STROBE_STATUS: u32 = CID_FLASH_CLASS_BASE + 5;
    pub const CID_FLASH_TIMEOUT: u32 = CID_FLASH_CLASS_BASE + 6;
    pub const CID_FLASH_INTENSITY: u32 = CID_FLASH_CLASS_BASE + 7;
    pub const CID_FLASH_TORCH_INTENSITY: u32 = CID_FLASH_CLASS_BASE + 8;
    pub const CID_FLASH_INDICATOR_INTENSITY: u32 = CID_FLASH_CLASS_BASE + 9;
    pub const CID_FLASH_FAULT: u32 = CID_FLASH_CLASS_BASE + 10;
    pub const FLASH_FAULT_OVER_VOLTAGE: u32 = 1;
    pub const FLASH_FAULT_TIMEOUT: u32 = 1 << 1;
    pub const FLASH_FAULT_OVER_TEMPERATURE: u32 = 1 << 2;
    pub const FLASH_FAULT_SHORT_CIRCUIT: u32 = 1 << 3;
    pub const FLASH_FAULT_OVER_CURRENT: u32 = 1 << 4;
    pub const FLASH_FAULT_INDICATOR: u32 = 1 << 5;
    pub const FLASH_FAULT_UNDER_VOLTAGE: u32 = 1 << 6;
    pub const FLASH_FAULT_INPUT_VOLTAGE: u32 = 1 << 7;
    pub const FLASH_FAULT_LED_OVER_TEMPERATURE: u32 = 1 << 8;
    pub const CID_FLASH_CHARGE: u32 = CID_FLASH_CLASS_BASE + 11;
    pub const CID_FLASH_READY: u32 = CID_FLASH_CLASS_BASE + 12;
    pub const CID_JPEG_CLASS_BASE: u32 = CLASS_JPEG | 0x900;
    pub const CID_JPEG_CLASS: u32 = CLASS_JPEG | 1;
    pub const CID_JPEG_CHROMA_SUBSAMPLING: u32 = CID_JPEG_CLASS_BASE + 1;
    pub const JPEG_CHROMA_SUBSAMPLING_444: u32 = 0;
    pub const JPEG_CHROMA_SUBSAMPLING_422: u32 = 1;
    pub const JPEG_CHROMA_SUBSAMPLING_420: u32 = 2;
    pub const JPEG_CHROMA_SUBSAMPLING_411: u32 = 3;
    pub const JPEG_CHROMA_SUBSAMPLING_410: u32 = 4;
    pub const JPEG_CHROMA_SUBSAMPLING_GRAY: u32 = 5;
    pub const CID_JPEG_RESTART_INTERVAL: u32 = CID_JPEG_CLASS_BASE + 2;
    pub const CID_JPEG_COMPRESSION_QUALITY: u32 = CID_JPEG_CLASS_BASE + 3;
    pub const CID_JPEG_ACTIVE_MARKER: u32 = CID_JPEG_CLASS_BASE + 4;
    pub const JPEG_ACTIVE_MARKER_APP0: u32 = 1;
    pub const JPEG_ACTIVE_MARKER_APP1: u32 = 1 << 1;
    pub const JPEG_ACTIVE_MARKER_COM: u32 = 1 << 16;
    pub const JPEG_ACTIVE_MARKER_DQT: u32 = 1 << 17;
    pub const JPEG_ACTIVE_MARKER_DHT: u32 = 1 << 18;
    pub const CID_IMAGE_SOURCE_CLASS_BASE: u32 = CLASS_IMAGE_SOURCE | 0x900;
    pub const CID_IMAGE_SOURCE_CLASS: u32 = CLASS_IMAGE_SOURCE | 1;
    pub const CID_VBLANK: u32 = CID_IMAGE_SOURCE_CLASS_BASE + 1;
    pub const CID_HBLANK: u32 = CID_IMAGE_SOURCE_CLASS_BASE + 2;
    pub const CID_ANALOGUE_GAIN: u32 = CID_IMAGE_SOURCE_CLASS_BASE + 3;
    pub const CID_TEST_PATTERN_RED: u32 = CID_IMAGE_SOURCE_CLASS_BASE + 4;
    pub const CID_TEST_PATTERN_GREENR: u32 = CID_IMAGE_SOURCE_CLASS_BASE + 5;
    pub const CID_TEST_PATTERN_BLUE: u32 = CID_IMAGE_SOURCE_CLASS_BASE + 6;
    pub const CID_TEST_PATTERN_GREENB: u32 = CID_IMAGE_SOURCE_CLASS_BASE + 7;
    pub const CID_IMAGE_PROC_CLASS_BASE: u32 = CLASS_IMAGE_PROC | 0x900;
    pub const CID_IMAGE_PROC_CLASS: u32 = CLASS_IMAGE_PROC | 1;
    pub const CID_LINK_FREQ: u32 = CID_IMAGE_PROC_CLASS_BASE + 1;
    pub const CID_PIXEL_RATE: u32 = CID_IMAGE_PROC_CLASS_BASE + 2;
    pub const CID_TEST_PATTERN: u32 = CID_IMAGE_PROC_CLASS_BASE + 3;
    pub const CID_DV_CLASS_BASE: u32 = CLASS_DV | 0x900;
    pub const CID_DV_CLASS: u32 = CLASS_DV | 1;
    pub const CID_DV_TX_HOTPLUG: u32 = CID_DV_CLASS_BASE + 1;
    pub const CID_DV_TX_RXSENSE: u32 = CID_DV_CLASS_BASE + 2;
    pub const CID_DV_TX_EDID_PRESENT: u32 = CID_DV_CLASS_BASE + 3;
    pub const CID_DV_TX_MODE: u32 = CID_DV_CLASS_BASE + 4;
    pub const DV_TX_MODE_DVI_D: u32 = 0;
    pub const DV_TX_MODE_HDMI: u32 = 1;
    pub const CID_DV_TX_RGB_RANGE: u32 = CID_DV_CLASS_BASE + 5;
    pub const DV_RGB_RANGE_AUTO: u32 = 0;
    pub const DV_RGB_RANGE_LIMITED: u32 = 1;
    pub const DV_RGB_RANGE_FULL: u32 = 2;
    pub const CID_DV_RX_POWER_PRESENT: u32 = CID_DV_CLASS_BASE + 100;
    pub const CID_DV_RX_RGB_RANGE: u32 = CID_DV_CLASS_BASE + 101;
    pub const CID_FM_RX_CLASS_BASE: u32 = CLASS_FM_RX | 0x900;
    pub const CID_FM_RX_CLASS: u32 = CLASS_FM_RX | 1;
    pub const CID_TUNE_DEEMPHASIS: u32 = CID_FM_RX_CLASS_BASE + 1;
    pub const DEEMPHASIS_DISABLED: u32 = PREEMPHASIS_DISABLED;
    pub const DEEMPHASIS_50_uS: u32 = PREEMPHASIS_50_uS;
    pub const DEEMPHASIS_75_uS: u32 = PREEMPHASIS_75_uS;
    pub const CID_RDS_RECEPTION: u32 = CID_FM_RX_CLASS_BASE + 2;
    pub const CID_RDS_RX_PTY: u32 = CID_FM_RX_CLASS_BASE + 3;
    pub const CID_RDS_RX_PS_NAME: u32 = CID_FM_RX_CLASS_BASE + 4;
    pub const CID_RDS_RX_RADIO_TEXT: u32 = CID_FM_RX_CLASS_BASE + 5;
    pub const CID_RDS_RX_TRAFFIC_ANNOUNCEMENT: u32 = CID_FM_RX_CLASS_BASE + 6;
    pub const CID_RDS_RX_TRAFFIC_PROGRAM: u32 = CID_FM_RX_CLASS_BASE + 7;
    pub const CID_RDS_RX_MUSIC_SPEECH: u32 = CID_FM_RX_CLASS_BASE + 8;
    pub const CID_RF_TUNER_CLASS_BASE: u32 = CLASS_RF_TUNER | 0x900;
    pub const CID_RF_TUNER_CLASS: u32 = CLASS_RF_TUNER | 1;
    pub const CID_RF_TUNER_BANDWIDTH_AUTO: u32 = CID_RF_TUNER_CLASS_BASE + 11;
    pub const CID_RF_TUNER_BANDWIDTH: u32 = CID_RF_TUNER_CLASS_BASE + 12;
    pub const CID_RF_TUNER_LNA_GAIN_AUTO: u32 = CID_RF_TUNER_CLASS_BASE + 41;
    pub const CID_RF_TUNER_LNA_GAIN: u32 = CID_RF_TUNER_CLASS_BASE + 42;
    pub const CID_RF_TUNER_MIXER_GAIN_AUTO: u32 = CID_RF_TUNER_CLASS_BASE + 51;
    pub const CID_RF_TUNER_MIXER_GAIN: u32 = CID_RF_TUNER_CLASS_BASE + 52;
    pub const CID_RF_TUNER_IF_GAIN_AUTO: u32 = CID_RF_TUNER_CLASS_BASE + 61;
    pub const CID_RF_TUNER_IF_GAIN: u32 = CID_RF_TUNER_CLASS_BASE + 62;
    pub const CID_RF_TUNER_PLL_LOCK: u32 = CID_RF_TUNER_CLASS_BASE + 91;
    pub const CID_DETECT_CLASS_BASE: u32 = CLASS_DETECT | 0x900;
    pub const CID_DETECT_CLASS: u32 = CLASS_DETECT | 1;
    pub const CID_DETECT_MD_MODE: u32 = CID_DETECT_CLASS_BASE + 1;
    pub const DETECT_MD_MODE_DISABLED: u32 = 0;
    pub const DETECT_MD_MODE_GLOBAL: u32 = 1;
    pub const DETECT_MD_MODE_THRESHOLD_GRID: u32 = 2;
    pub const DETECT_MD_MODE_REGION_GRID: u32 = 3;
    pub const CID_DETECT_MD_GLOBAL_THRESHOLD: u32 = CID_DETECT_CLASS_BASE + 2;
    pub const CID_DETECT_MD_THRESHOLD_GRID: u32 = CID_DETECT_CLASS_BASE + 3;
    pub const CID_DETECT_MD_REGION_GRID: u32 = CID_DETECT_CLASS_BASE + 4;
}

// IOCTL codes.
pub const VIDIOC_ENUM_FMT: usize = 3225441794;
pub const VIDIOC_ENUM_FRAMEINTERVALS: usize = 3224655435;
pub const VIDIOC_ENUM_FRAMESIZES: usize = 3224131146;
pub const VIDIOC_G_CTRL: usize = 3221771803;
pub const VIDIOC_QUERYCTRL: usize = 3225703972;
pub const VIDIOC_QUERY_EXT_CTRL: usize = 3236451943;
pub const VIDIOC_QUERYMENU: usize = 3224131109;
pub const VIDIOC_REQBUFS: usize = 3222558216;
pub const VIDIOC_S_PARM: usize = 3234616854;
#[cfg(target_os = "linux")]
pub const VIDIOC_STREAMOFF: usize = 1074026003;
#[cfg(target_os = "freebsd")]
pub const VIDIOC_STREAMOFF: usize = 2147767827;
#[cfg(target_os = "linux")]
pub const VIDIOC_STREAMON: usize = 1074026002;
#[cfg(target_os = "freebsd")]
pub const VIDIOC_STREAMON: usize = 2147767826;

#[cfg(target_pointer_width = "64")]
pub const VIDIOC_DQBUF: usize = 3227014673;
#[cfg(target_pointer_width = "32")]
pub const VIDIOC_DQBUF: usize = 3225703953;

#[cfg(target_pointer_width = "64")]
pub const VIDIOC_QBUF: usize = 3227014671;
#[cfg(target_pointer_width = "32")]
pub const VIDIOC_QBUF: usize = 3225703951;

#[cfg(target_pointer_width = "64")]
pub const VIDIOC_QUERYBUF: usize = 3227014665;
#[cfg(target_pointer_width = "32")]
pub const VIDIOC_QUERYBUF: usize = 3225703945;

#[cfg(target_pointer_width = "64")]
pub const VIDIOC_S_FMT: usize = 3234878981;
#[cfg(target_pointer_width = "32")]
pub const VIDIOC_S_FMT: usize = 3234616837;

#[cfg(target_pointer_width = "64")]
pub const VIDIOC_G_EXT_CTRLS: usize = 3223344711;
#[cfg(target_pointer_width = "32")]
pub const VIDIOC_G_EXT_CTRLS: usize = 3222820423;

#[cfg(target_pointer_width = "64")]
pub const VIDIOC_S_EXT_CTRLS: usize = 3223344712;
#[cfg(target_pointer_width = "32")]
pub const VIDIOC_S_EXT_CTRLS: usize = 3222820424;

#[test]
fn test_sizes() {
    if cfg!(target_pointer_width = "64") {
        assert_eq!(mem::size_of::<Format>(), 208);
    } else {
        assert_eq!(mem::size_of::<Format>(), 204);
    }

    if cfg!(target_pointer_width = "64") {
        assert_eq!(mem::size_of::<Buffer>(), 88);
    } else {
        assert_eq!(mem::size_of::<Buffer>(), 68);
    }

    assert_eq!(mem::size_of::<StreamParm>(), 204);
    assert_eq!(mem::size_of::<FmtDesc>(), 64);
    assert_eq!(mem::size_of::<Frmsizeenum>(), 44);
    assert_eq!(mem::size_of::<Frmivalenum>(), 52);
    assert_eq!(mem::size_of::<QueryCtrl>(), 68);
    assert_eq!(mem::size_of::<QueryExtCtrl>(), 232);
    assert_eq!(mem::size_of::<QueryMenu>(), 44);
    assert_eq!(mem::size_of::<Control>(), 8);
    assert_eq!(mem::size_of::<ExtControl>(), 20);

    if cfg!(target_pointer_width = "64") {
        assert_eq!(mem::size_of::<ExtControls<'_>>(), 32);
    } else {
        assert_eq!(mem::size_of::<ExtControls<'_>>(), 24);
    }
}
