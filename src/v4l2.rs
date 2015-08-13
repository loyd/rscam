use std::ffi::CString;
use std::os::unix::io::RawFd;
use std::{io, mem, usize};

// C types and constants.
use libc::{c_void, c_ulong, size_t, off_t};
use libc::timeval as Timeval;
use libc::{O_RDWR, PROT_READ, PROT_WRITE};
use libc::consts::os::posix88::{MAP_SHARED};


#[cfg(not(feature = "no_wrapper"))]
mod ll {
    use std::os::unix::io::RawFd;
    use libc::{c_void, c_char, c_int, c_ulong, size_t, off_t};

    pub use self::v4l2_open as open;
    pub use self::v4l2_close as close;
    pub use self::v4l2_ioctl as ioctl;
    pub use self::v4l2_mmap as mmap;
    pub use self::v4l2_munmap as munmap;

    #[link(name = "v4l2")]
    extern {
        pub fn v4l2_open(file: *const c_char, flags: c_int, arg: c_int) -> RawFd;
        pub fn v4l2_close(fd: RawFd) -> c_int;
        pub fn v4l2_ioctl(fd: RawFd, request: c_ulong, argp: *mut c_void) -> c_int;
        pub fn v4l2_mmap(start: *mut c_void, length: size_t, prot: c_int,
                     flags: c_int, fd: RawFd, offset: off_t) -> *mut c_void;
        pub fn v4l2_munmap(start: *mut c_void, length: size_t) -> c_int;
    }
}

#[cfg(feature = "no_wrapper")]
mod ll {
    use std::os::unix::io::RawFd;
    use libc::{c_void, c_int, c_ulong};

    pub use libc::{open, close, mmap, munmap};

    extern {
        pub fn ioctl(fd: RawFd, request: c_ulong, argp: *mut c_void) -> c_int;
    }
}

macro_rules! check_io(
    ($cond:expr) =>
        (try!(if $cond { Ok(()) } else { Err(io::Error::last_os_error()) }))
);

pub fn open(file: &str) -> io::Result<RawFd> {
    let c_str = try!(CString::new(file));
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

pub fn xioctl_valid<T>(fd: RawFd, request: usize, arg: &mut T) ->io::Result<bool> {
    match xioctl(fd, request, arg) {
        Ok(_) => Ok(true),
        Err(ref err) if err.kind() == io::ErrorKind::InvalidInput => Ok(false),
        Err(err) => Err(err)
    }
}

pub struct MappedRegion {
    pub ptr: *mut u8,
    pub len: usize
}

// Instead of unstable `Unique<u8>`.
unsafe impl Send for MappedRegion {}
unsafe impl Sync for MappedRegion {}

impl Drop for MappedRegion {
    fn drop(&mut self) {
        unsafe { ll::munmap(*self.ptr as *mut c_void, self.len as size_t); }
    }
}

pub fn mmap(length: usize, fd: RawFd, offset: usize) -> io::Result<MappedRegion> {
    let ptr = unsafe { ll::mmap(0 as *mut c_void, length as size_t, PROT_READ|PROT_WRITE,
                                MAP_SHARED, fd, offset as off_t)};

    check_io!(ptr as usize != usize::MAX);
    Ok(MappedRegion { ptr: ptr as *mut u8, len: length })
}

#[repr(C)]
pub struct Format {
    pub ftype: u32,
    #[cfg(target_pointer_width = "64")]
    padding: u32,
    pub fmt: PixFormat,
    space: [u8; 156]
}

impl Format {
    #[cfg(target_pointer_width = "64")]
    pub fn new(resolution: (u32, u32), fourcc: u32, field: u32) -> Format {
        Format {
            ftype: BUF_TYPE_VIDEO_CAPTURE,
            padding: 0,
            fmt: PixFormat::new(resolution, fourcc, field),
            space: [0; 156]
        }
    }

    #[cfg(target_pointer_width = "32")]
    pub fn new(resolution: (u32, u32), fourcc: u32, field: u32) -> Format {
        Format {
            ftype: BUF_TYPE_VIDEO_CAPTURE,
            fmt: PixFormat::new(resolution, fourcc, field),
            space: [0; 156]
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
    pub quantization: u32
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
    reserved: [u32; 2]
}

impl RequestBuffers {
    pub fn new(nbuffers: u32) -> RequestBuffers {
        RequestBuffers {
            count: nbuffers,
            btype: BUF_TYPE_VIDEO_CAPTURE,
            memory: MEMORY_MMAP,
            reserved: [0; 2]
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
    pub m: usize,   // offset (__u32) or userptr (ulong)
    pub length: u32,
    pub input: u32,
    reserved: u32
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
    pub userbits: [u8; 4]
}

#[repr(C)]
pub struct FmtDesc {
    pub index: u32,
    pub ftype: u32,
    pub flags: u32,
    pub description: [u8; 32],
    pub pixelformat: u32,
    reserved: [u32; 4]
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
    space: [u8; 160]
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
    reserved: [u32; 4]
}

#[repr(C)]
pub struct Fract {
    pub numerator: u32,
    pub denominator: u32
}

#[repr(C)]
pub struct Frmsizeenum {
    pub index: u32,
    pub pixelformat: u32,
    pub ftype: u32,
    data: [u32; 6],
    reserved: [u32; 2]
}

impl Frmsizeenum {
    pub fn new(fourcc: u32) -> Frmsizeenum {
        let mut size: Frmsizeenum = unsafe { mem::zeroed() };
        size.pixelformat = fourcc;
        size
    }

    pub fn discrete(&mut self) -> &mut FrmsizeDiscrete {
        unsafe { mem::transmute(self.data.as_mut_ptr()) }
    }

    pub fn stepwise(&mut self) -> &mut FrmsizeStepwise {
        unsafe { mem::transmute(self.data.as_mut_ptr()) }
    }
}

#[repr(C)]
pub struct FrmsizeDiscrete {
    pub width: u32,
    pub height: u32
}

#[repr(C)]
pub struct FrmsizeStepwise {
    pub min_width: u32,
    pub max_width: u32,
    pub step_width: u32,
    pub min_height: u32,
    pub max_height: u32,
    pub step_height: u32
}

#[repr(C)]
pub struct Frmivalenum {
    pub index: u32,
    pub pixelformat: u32,
    pub width: u32,
    pub height: u32,
    pub ftype: u32,
    data: [u32; 6],
    reserved: [u32; 2]
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
        unsafe { mem::transmute(self.data.as_mut_ptr()) }
    }

    pub fn stepwise(&mut self) -> &mut FrmivalStepwise {
        unsafe { mem::transmute(self.data.as_mut_ptr()) }
    }
}

#[repr(C)]
pub struct FrmivalStepwise {
    pub min: Fract,
    pub max: Fract,
    pub step: Fract
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
    reserved: [u32; 2]
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
    reserved: [u32; 32]
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
    pub name: [u8; 32],
	reserved: u32
}

impl QueryMenu {
    pub fn new(id: u32) -> QueryMenu {
        let mut menu: QueryMenu = unsafe { mem::zeroed() };
        menu.id = id;
        menu
    }

    pub fn value(&mut self) -> &mut i64 {
        unsafe { mem::transmute(self.name.as_mut_ptr()) }
    }
}

#[repr(C)]
pub struct Control {
    pub id: u32,
    pub value: i32
}

impl Control {
    pub fn new(id: u32) -> Control {
        Control { id: id, value: 0 }
    }
}

#[repr(C, packed)]
pub struct ExtControl {
    pub id: u32,
    pub size: u32,
    reserved: u32,
    pub value: i64
}

impl ExtControl {
    pub fn new(id: u32, size: u32) -> ExtControl {
        ExtControl { id: id, size: size, reserved: 0, value: 0 }
    }
}

#[repr(C)]
pub struct ExtControls<'a> {
    pub ctrl_class: u32,
    pub count: u32,
    pub error_idx: u32,
    reserved: [u32; 2],
    pub controls: &'a mut ExtControl
}

impl<'a> ExtControls<'a> {
    pub fn new(class: u32, ctrl: &mut ExtControl) -> ExtControls {
        ExtControls { ctrl_class: class, count: 1, error_idx: 0, reserved: [0; 2], controls: ctrl }
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
pub const CTRL_TYPE_STRING: u32 = 7;
pub const CTRL_TYPE_BITMASK: u32 = 8;
pub const CTRL_TYPE_INTEGER_MENU: u32 = 9;

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
    pub const FLAG_DISABLED: u32 = 0x0001;
    pub const FLAG_GRABBED: u32 = 0x0002;
    pub const FLAG_READ_ONLY: u32 = 0x0004;
    pub const FLAG_UPDATE: u32 = 0x0008;
    pub const FLAG_INACTIVE: u32 = 0x0010;
    pub const FLAG_SLIDER: u32 = 0x0020;
    pub const FLAG_WRITE_ONLY: u32 = 0x0040;
    pub const FLAG_VOLATILE: u32 = 0x0080;
    pub const FLAG_HAS_PAYLOAD: u32 = 0x0100;
    pub const FLAG_EXECUTE_ON_WRITE: u32 = 0x0200;

    // Control classses.
    pub const CLASS_ALL: u32 = 0;
    /// User controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/control.html).
    pub const CLASS_USER: u32 = 0x00980000;
    /// MPEG compression controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#mpeg-controls).
    pub const CLASS_MPEG: u32 = 0x00990000;
    /// Camera controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#camera-controls).
    pub const CLASS_CAMERA: u32 = 0x009a0000;
    /// FM Transmitter controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#fm-tx-controls).
    pub const CLASS_FM_TX: u32 = 0x009b0000;
    /// Flash device controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#flash-controls).
    pub const CLASS_FLASH: u32 = 0x009c0000;
    /// JPEG compression controls.
    /// [details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#jpeg-controls).
    pub const CLASS_JPEG: u32 = 0x009d0000;
    /// low-level controls of image source.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#image-source-controls).
    pub const CLASS_IMAGE_SOURCE: u32 = 0x009e0000;
    /// Image processing controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#image-process-controls).
    pub const CLASS_IMAGE_PROC: u32 = 0x009f0000;
    /// Digital Video controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#dv-controls).
    pub const CLASS_DV: u32 = 0x00a00000;
    /// FM Receiver controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#fm-rx-controls).
    pub const CLASS_FM_RX: u32 = 0x00a10000;
    /// RF tuner controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#rf-tuner-controls).
    pub const CLASS_RF_TUNER: u32 = 0x00a20000;
    /// Motion or object detection controls.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/extended-controls.html#detect-controls).
    pub const CLASS_DETECT: u32 = 0x00a30000;

    // Control ids.
    pub const CID_BASE: u32 = CLASS_USER | 0x900;
    pub const CID_USER_BASE: u32 = CID_BASE;
    pub const CID_USER_CLASS: u32 = CLASS_USER | 1;
    pub const CID_BRIGHTNESS: u32 = CID_BASE + 0;
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
    pub const CID_HUE_AUTO: u32 = CID_BASE + 25;
    pub const CID_WHITE_BALANCE_TEMPERATURE: u32 = CID_BASE + 26;
    pub const CID_SHARPNESS: u32 = CID_BASE + 27;
    pub const CID_BACKLIGHT_COMPENSATION: u32 = CID_BASE + 28;
    pub const CID_CHROMA_AGC: u32 = CID_BASE + 29;
    pub const CID_COLOR_KILLER: u32 = CID_BASE + 30;
    pub const CID_COLORFX: u32 = CID_BASE + 31;
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
    pub const CID_MPEG_STREAM_TYPE: u32 = CID_MPEG_BASE + 0;
    pub const CID_MPEG_STREAM_PID_PMT: u32 = CID_MPEG_BASE + 1;
    pub const CID_MPEG_STREAM_PID_AUDIO: u32 = CID_MPEG_BASE + 2;
    pub const CID_MPEG_STREAM_PID_VIDEO: u32 = CID_MPEG_BASE + 3;
    pub const CID_MPEG_STREAM_PID_PCR: u32 = CID_MPEG_BASE + 4;
    pub const CID_MPEG_STREAM_PES_ID_AUDIO: u32 = CID_MPEG_BASE + 5;
    pub const CID_MPEG_STREAM_PES_ID_VIDEO: u32 = CID_MPEG_BASE + 6;
    pub const CID_MPEG_STREAM_VBI_FMT: u32 = CID_MPEG_BASE + 7;
    pub const CID_MPEG_AUDIO_SAMPLING_FREQ: u32 = CID_MPEG_BASE + 100;
    pub const CID_MPEG_AUDIO_ENCODING: u32 = CID_MPEG_BASE + 101;
    pub const CID_MPEG_AUDIO_L1_BITRATE: u32 = CID_MPEG_BASE + 102;
    pub const CID_MPEG_AUDIO_L2_BITRATE: u32 = CID_MPEG_BASE + 103;
    pub const CID_MPEG_AUDIO_L3_BITRATE: u32 = CID_MPEG_BASE + 104;
    pub const CID_MPEG_AUDIO_MODE: u32 = CID_MPEG_BASE + 105;
    pub const CID_MPEG_AUDIO_MODE_EXTENSION: u32 = CID_MPEG_BASE + 106;
    pub const CID_MPEG_AUDIO_EMPHASIS: u32 = CID_MPEG_BASE + 107;
    pub const CID_MPEG_AUDIO_CRC: u32 = CID_MPEG_BASE + 108;
    pub const CID_MPEG_AUDIO_MUTE: u32 = CID_MPEG_BASE + 109;
    pub const CID_MPEG_AUDIO_AAC_BITRATE: u32 = CID_MPEG_BASE + 110;
    pub const CID_MPEG_AUDIO_AC3_BITRATE: u32 = CID_MPEG_BASE + 111;
    pub const CID_MPEG_AUDIO_DEC_PLAYBACK: u32 = CID_MPEG_BASE + 112;
    pub const CID_MPEG_AUDIO_DEC_MULTILINGUAL_PLAYBACK: u32 = CID_MPEG_BASE + 113;
    pub const CID_MPEG_VIDEO_ENCODING: u32 = CID_MPEG_BASE + 200;
    pub const CID_MPEG_VIDEO_ASPECT: u32 = CID_MPEG_BASE + 201;
    pub const CID_MPEG_VIDEO_B_FRAMES: u32 = CID_MPEG_BASE + 202;
    pub const CID_MPEG_VIDEO_GOP_SIZE: u32 = CID_MPEG_BASE + 203;
    pub const CID_MPEG_VIDEO_GOP_CLOSURE: u32 = CID_MPEG_BASE + 204;
    pub const CID_MPEG_VIDEO_PULLDOWN: u32 = CID_MPEG_BASE + 205;
    pub const CID_MPEG_VIDEO_BITRATE_MODE: u32 = CID_MPEG_BASE + 206;
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
    pub const CID_MPEG_VIDEO_MAX_REF_PIC: u32 = CID_MPEG_BASE + 217;
    pub const CID_MPEG_VIDEO_MB_RC_ENABLE: u32 = CID_MPEG_BASE + 218;
    pub const CID_MPEG_VIDEO_MULTI_SLICE_MAX_BYTES: u32 = CID_MPEG_BASE + 219;
    pub const CID_MPEG_VIDEO_MULTI_SLICE_MAX_MB: u32 = CID_MPEG_BASE + 220;
    pub const CID_MPEG_VIDEO_MULTI_SLICE_MODE: u32 = CID_MPEG_BASE + 221;
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
    pub const CID_MPEG_VIDEO_H264_I_PERIOD: u32 = CID_MPEG_BASE + 358;
    pub const CID_MPEG_VIDEO_H264_LEVEL: u32 = CID_MPEG_BASE + 359;
    pub const CID_MPEG_VIDEO_H264_LOOP_FILTER_ALPHA: u32 = CID_MPEG_BASE + 360;
    pub const CID_MPEG_VIDEO_H264_LOOP_FILTER_BETA: u32 = CID_MPEG_BASE + 361;
    pub const CID_MPEG_VIDEO_H264_LOOP_FILTER_MODE: u32 = CID_MPEG_BASE + 362;
    pub const CID_MPEG_VIDEO_H264_PROFILE: u32 = CID_MPEG_BASE + 363;
    pub const CID_MPEG_VIDEO_H264_VUI_EXT_SAR_HEIGHT: u32 = CID_MPEG_BASE + 364;
    pub const CID_MPEG_VIDEO_H264_VUI_EXT_SAR_WIDTH: u32 = CID_MPEG_BASE + 365;
    pub const CID_MPEG_VIDEO_H264_VUI_SAR_ENABLE: u32 = CID_MPEG_BASE + 366;
    pub const CID_MPEG_VIDEO_H264_VUI_SAR_IDC: u32 = CID_MPEG_BASE + 367;
    pub const CID_MPEG_VIDEO_H264_SEI_FRAME_PACKING: u32 = CID_MPEG_BASE + 368;
    pub const CID_MPEG_VIDEO_H264_SEI_FP_CURRENT_FRAME_0: u32 = CID_MPEG_BASE + 369;
    pub const CID_MPEG_VIDEO_H264_SEI_FP_ARRANGEMENT_TYPE: u32 = CID_MPEG_BASE + 370;
    pub const CID_MPEG_VIDEO_H264_FMO: u32 = CID_MPEG_BASE + 371;
    pub const CID_MPEG_VIDEO_H264_FMO_MAP_TYPE: u32 = CID_MPEG_BASE + 372;
    pub const CID_MPEG_VIDEO_H264_FMO_SLICE_GROUP: u32 = CID_MPEG_BASE + 373;
    pub const CID_MPEG_VIDEO_H264_FMO_CHANGE_DIRECTION: u32 = CID_MPEG_BASE + 374;
    pub const CID_MPEG_VIDEO_H264_FMO_CHANGE_RATE: u32 = CID_MPEG_BASE + 375;
    pub const CID_MPEG_VIDEO_H264_FMO_RUN_LENGTH: u32 = CID_MPEG_BASE + 376;
    pub const CID_MPEG_VIDEO_H264_ASO: u32 = CID_MPEG_BASE + 377;
    pub const CID_MPEG_VIDEO_H264_ASO_SLICE_ORDER: u32 = CID_MPEG_BASE + 378;
    pub const CID_MPEG_VIDEO_H264_HIERARCHICAL_CODING: u32 = CID_MPEG_BASE + 379;
    pub const CID_MPEG_VIDEO_H264_HIERARCHICAL_CODING_TYPE: u32 = CID_MPEG_BASE + 380;
    pub const CID_MPEG_VIDEO_H264_HIERARCHICAL_CODING_LAYER: u32 = CID_MPEG_BASE + 381;
    pub const CID_MPEG_VIDEO_H264_HIERARCHICAL_CODING_LAYER_QP: u32 = CID_MPEG_BASE + 382;
    pub const CID_MPEG_VIDEO_MPEG4_I_FRAME_QP: u32 = CID_MPEG_BASE + 400;
    pub const CID_MPEG_VIDEO_MPEG4_P_FRAME_QP: u32 = CID_MPEG_BASE + 401;
    pub const CID_MPEG_VIDEO_MPEG4_B_FRAME_QP: u32 = CID_MPEG_BASE + 402;
    pub const CID_MPEG_VIDEO_MPEG4_MIN_QP: u32 = CID_MPEG_BASE + 403;
    pub const CID_MPEG_VIDEO_MPEG4_MAX_QP: u32 = CID_MPEG_BASE + 404;
    pub const CID_MPEG_VIDEO_MPEG4_LEVEL: u32 = CID_MPEG_BASE + 405;
    pub const CID_MPEG_VIDEO_MPEG4_PROFILE: u32 = CID_MPEG_BASE + 406;
    pub const CID_MPEG_VIDEO_MPEG4_QPEL: u32 = CID_MPEG_BASE + 407;
    pub const CID_MPEG_VIDEO_VPX_NUM_PARTITIONS: u32 = CID_MPEG_BASE + 500;
    pub const CID_MPEG_VIDEO_VPX_IMD_DISABLE_4X4: u32 = CID_MPEG_BASE + 501;
    pub const CID_MPEG_VIDEO_VPX_NUM_REF_FRAMES: u32 = CID_MPEG_BASE + 502;
    pub const CID_MPEG_VIDEO_VPX_FILTER_LEVEL: u32 = CID_MPEG_BASE + 503;
    pub const CID_MPEG_VIDEO_VPX_FILTER_SHARPNESS: u32 = CID_MPEG_BASE + 504;
    pub const CID_MPEG_VIDEO_VPX_GOLDEN_FRAME_REF_PERIOD: u32 = CID_MPEG_BASE + 505;
    pub const CID_MPEG_VIDEO_VPX_GOLDEN_FRAME_SEL: u32 = CID_MPEG_BASE + 506;
    pub const CID_MPEG_VIDEO_VPX_MIN_QP: u32 = CID_MPEG_BASE + 507;
    pub const CID_MPEG_VIDEO_VPX_MAX_QP: u32 = CID_MPEG_BASE + 508;
    pub const CID_MPEG_VIDEO_VPX_I_FRAME_QP: u32 = CID_MPEG_BASE + 509;
    pub const CID_MPEG_VIDEO_VPX_P_FRAME_QP: u32 = CID_MPEG_BASE + 510;
    pub const CID_MPEG_VIDEO_VPX_PROFILE: u32 = CID_MPEG_BASE + 511;
    pub const CID_MPEG_CX2341X_BASE: u32 = CLASS_MPEG | 0x1000;
    pub const CID_MPEG_CX2341X_VIDEO_SPATIAL_FILTER_MODE: u32 = CID_MPEG_CX2341X_BASE + 0;
    pub const CID_MPEG_CX2341X_VIDEO_SPATIAL_FILTER: u32 = CID_MPEG_CX2341X_BASE + 1;
    pub const CID_MPEG_CX2341X_VIDEO_LUMA_SPATIAL_FILTER_TYPE: u32 = CID_MPEG_CX2341X_BASE + 2;
    pub const CID_MPEG_CX2341X_VIDEO_CHROMA_SPATIAL_FILTER_TYPE: u32 = CID_MPEG_CX2341X_BASE + 3;
    pub const CID_MPEG_CX2341X_VIDEO_TEMPORAL_FILTER_MODE: u32 = CID_MPEG_CX2341X_BASE + 4;
    pub const CID_MPEG_CX2341X_VIDEO_TEMPORAL_FILTER: u32 = CID_MPEG_CX2341X_BASE + 5;
    pub const CID_MPEG_CX2341X_VIDEO_MEDIAN_FILTER_TYPE: u32 = CID_MPEG_CX2341X_BASE + 6;
    pub const CID_MPEG_CX2341X_VIDEO_LUMA_MEDIAN_FILTER_BOTTOM: u32 = CID_MPEG_CX2341X_BASE + 7;
    pub const CID_MPEG_CX2341X_VIDEO_LUMA_MEDIAN_FILTER_TOP: u32 = CID_MPEG_CX2341X_BASE + 8;
    pub const CID_MPEG_CX2341X_VIDEO_CHROMA_MEDIAN_FILTER_BOTTOM: u32 = CID_MPEG_CX2341X_BASE + 9;
    pub const CID_MPEG_CX2341X_VIDEO_CHROMA_MEDIAN_FILTER_TOP: u32 = CID_MPEG_CX2341X_BASE + 10;
    pub const CID_MPEG_CX2341X_STREAM_INSERT_NAV_PACKETS: u32 = CID_MPEG_CX2341X_BASE + 11;
    pub const CID_MPEG_MFC51_BASE: u32 = CLASS_MPEG | 0x1100;
    pub const CID_MPEG_MFC51_VIDEO_DECODER_H264_DISPLAY_DELAY: u32 = CID_MPEG_MFC51_BASE + 0;
    pub const CID_MPEG_MFC51_VIDEO_DECODER_H264_DISPLAY_DELAY_ENABLE: u32 = CID_MPEG_MFC51_BASE + 1;
    pub const CID_MPEG_MFC51_VIDEO_FRAME_SKIP_MODE: u32 = CID_MPEG_MFC51_BASE + 2;
    pub const CID_MPEG_MFC51_VIDEO_FORCE_FRAME_TYPE: u32 = CID_MPEG_MFC51_BASE + 3;
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
    pub const CID_WIDE_DYNAMIC_RANGE: u32 = CID_CAMERA_CLASS_BASE + 21;
    pub const CID_IMAGE_STABILIZATION: u32 = CID_CAMERA_CLASS_BASE + 22;
    pub const CID_ISO_SENSITIVITY: u32 = CID_CAMERA_CLASS_BASE + 23;
    pub const CID_ISO_SENSITIVITY_AUTO: u32 = CID_CAMERA_CLASS_BASE + 24;
    pub const CID_EXPOSURE_METERING: u32 = CID_CAMERA_CLASS_BASE + 25;
    pub const CID_SCENE_MODE: u32 = CID_CAMERA_CLASS_BASE + 26;
    pub const CID_3A_LOCK: u32 = CID_CAMERA_CLASS_BASE + 27;
    pub const CID_AUTO_FOCUS_START: u32 = CID_CAMERA_CLASS_BASE + 28;
    pub const CID_AUTO_FOCUS_STOP: u32 = CID_CAMERA_CLASS_BASE + 29;
    pub const CID_AUTO_FOCUS_STATUS: u32 = CID_CAMERA_CLASS_BASE + 30;
    pub const CID_AUTO_FOCUS_RANGE: u32 = CID_CAMERA_CLASS_BASE + 31;
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
    pub const CID_TUNE_POWER_LEVEL: u32 = CID_FM_TX_CLASS_BASE + 113;
    pub const CID_TUNE_ANTENNA_CAPACITOR: u32 = CID_FM_TX_CLASS_BASE + 114;
    pub const CID_FLASH_CLASS_BASE: u32 = CLASS_FLASH | 0x900;
    pub const CID_FLASH_CLASS: u32 = CLASS_FLASH | 1;
    pub const CID_FLASH_LED_MODE: u32 = CID_FLASH_CLASS_BASE + 1;
    pub const CID_FLASH_STROBE_SOURCE: u32 = CID_FLASH_CLASS_BASE + 2;
    pub const CID_FLASH_STROBE: u32 = CID_FLASH_CLASS_BASE + 3;
    pub const CID_FLASH_STROBE_STOP: u32 = CID_FLASH_CLASS_BASE + 4;
    pub const CID_FLASH_STROBE_STATUS: u32 = CID_FLASH_CLASS_BASE + 5;
    pub const CID_FLASH_TIMEOUT: u32 = CID_FLASH_CLASS_BASE + 6;
    pub const CID_FLASH_INTENSITY: u32 = CID_FLASH_CLASS_BASE + 7;
    pub const CID_FLASH_TORCH_INTENSITY: u32 = CID_FLASH_CLASS_BASE + 8;
    pub const CID_FLASH_INDICATOR_INTENSITY: u32 = CID_FLASH_CLASS_BASE + 9;
    pub const CID_FLASH_FAULT: u32 = CID_FLASH_CLASS_BASE + 10;
    pub const CID_FLASH_CHARGE: u32 = CID_FLASH_CLASS_BASE + 11;
    pub const CID_FLASH_READY: u32 = CID_FLASH_CLASS_BASE + 12;
    pub const CID_JPEG_CLASS_BASE: u32 = CLASS_JPEG | 0x900;
    pub const CID_JPEG_CLASS: u32 = CLASS_JPEG | 1;
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
    pub const CID_DV_TX_RGB_RANGE: u32 = CID_DV_CLASS_BASE + 5;
    pub const CID_DV_RX_RGB_RANGE: u32 = CID_DV_CLASS_BASE + 101;
    pub const CID_FM_RX_CLASS_BASE: u32 = CLASS_FM_RX | 0x900;
    pub const CID_FM_RX_CLASS: u32 = CLASS_FM_RX | 1;
    pub const CID_TUNE_DEEMPHASIS: u32 = CID_FM_RX_CLASS_BASE + 1;
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
    pub const CID_DETECT_MD_GLOBAL_THRESHOLD: u32 = CID_DETECT_CLASS_BASE + 2;
    pub const CID_DETECT_MD_THRESHOLD_GRID: u32 = CID_DETECT_CLASS_BASE + 3;
    pub const CID_DETECT_MD_REGION_GRID: u32 = CID_DETECT_CLASS_BASE + 4;
}

// IOCTL codes.
pub const VIDIOC_ENUM_FMT: usize = 3225441794;
pub const VIDIOC_ENUM_FRAMEINTERVALS: usize = 3224655435;
pub const VIDIOC_ENUM_FRAMESIZES: usize = 3224131146;
pub const VIDIOC_G_CTRL: usize = 3221771803;
pub const VIDIOC_G_EXT_CTRLS: usize = 3223344711;
pub const VIDIOC_QUERYCTRL: usize = 3225703972;
pub const VIDIOC_QUERY_EXT_CTRL: usize = 3236451943;
pub const VIDIOC_QUERYMENU: usize = 3224131109;
pub const VIDIOC_REQBUFS: usize = 3222558216;
pub const VIDIOC_S_EXT_CTRLS: usize = 3223344712;
pub const VIDIOC_S_PARM: usize = 3234616854;
pub const VIDIOC_STREAMOFF: usize = 1074026003;
pub const VIDIOC_STREAMON: usize = 1074026002;

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
    assert_eq!(mem::size_of::<ExtControls>(), 32);
}
