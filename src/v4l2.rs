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
    pub fn new() -> QueryCtrl {
        unsafe { mem::zeroed() }
    }
}

#[repr(C)]
#[packed]
pub struct QueryMenu {
	pub id: u32,
	pub index: u32,
    pub name: [u8; 32],
	reserved: u32
}

impl QueryMenu {
    pub fn new() -> QueryMenu {
        unsafe { mem::zeroed() }
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
    pub fn new() -> Control {
        unsafe { mem::zeroed() }
    }
}

pub const BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
pub const FMT_FLAG_COMPRESSED: u32 = 1;
pub const FMT_FLAG_EMULATED: u32 = 2;
pub const FRMIVAL_TYPE_DISCRETE: u32 = 1;
pub const FRMSIZE_TYPE_DISCRETE: u32 = 1;
pub const MEMORY_MMAP: u32 = 1;

// Control IDs.
pub const CID_BASE: u32 = 9963776;
pub const CID_BRIGHTNESS: u32 = (CID_BASE+0);
pub const CID_CONTRAST: u32 = (CID_BASE+1);
pub const CID_SATURATION: u32 = (CID_BASE+2);
pub const CID_HUE: u32 = (CID_BASE+3);
pub const CID_AUDIO_VOLUME: u32 = (CID_BASE+5);
pub const CID_AUDIO_BALANCE: u32 = (CID_BASE+6);
pub const CID_AUDIO_BASS: u32 = (CID_BASE+7);
pub const CID_AUDIO_TREBLE: u32 = (CID_BASE+8);
pub const CID_AUDIO_MUTE: u32 = (CID_BASE+9);
pub const CID_AUDIO_LOUDNESS: u32 = (CID_BASE+10);
pub const CID_BLACK_LEVEL: u32 = (CID_BASE+11);
pub const CID_AUTO_WHITE_BALANCE: u32 = (CID_BASE+12);
pub const CID_DO_WHITE_BALANCE: u32 = (CID_BASE+13);
pub const CID_RED_BALANCE: u32 = (CID_BASE+14);
pub const CID_BLUE_BALANCE: u32 = (CID_BASE+15);
pub const CID_GAMMA: u32 = (CID_BASE+16);
pub const CID_WHITENESS: u32 = (CID_GAMMA);
pub const CID_EXPOSURE: u32 = (CID_BASE+17);
pub const CID_AUTOGAIN: u32 = (CID_BASE+18);
pub const CID_GAIN: u32 = (CID_BASE+19);
pub const CID_HFLIP: u32 = (CID_BASE+20);
pub const CID_VFLIP: u32 = (CID_BASE+21);
pub const CID_POWER_LINE_FREQUENCY: u32 = (CID_BASE+24);
pub const CID_HUE_AUTO: u32 = (CID_BASE+25);
pub const CID_WHITE_BALANCE_TEMPERATURE: u32 = (CID_BASE+26);
pub const CID_SHARPNESS: u32 = (CID_BASE+27);
pub const CID_BACKLIGHT_COMPENSATION: u32 = (CID_BASE+28);
pub const CID_CHROMA_AGC: u32 = (CID_BASE+29);
pub const CID_COLOR_KILLER: u32 = (CID_BASE+30);
pub const CID_COLORFX: u32 = (CID_BASE+31);
pub const CID_AUTOBRIGHTNESS: u32 = (CID_BASE+32);
pub const CID_BAND_STOP_FILTER: u32 = (CID_BASE+33);
pub const CID_ROTATE: u32 = (CID_BASE+34);
pub const CID_BG_COLOR: u32 = (CID_BASE+35);
pub const CID_CHROMA_GAIN: u32 = (CID_BASE+36);
pub const CID_ILLUMINATORS_1: u32 = (CID_BASE+37);
pub const CID_ILLUMINATORS_2: u32 = (CID_BASE+38);
pub const CID_MIN_BUFFERS_FOR_CAPTURE: u32 = (CID_BASE+39);
pub const CID_MIN_BUFFERS_FOR_OUTPUT: u32 = (CID_BASE+40);
pub const CID_ALPHA_COMPONENT: u32 = (CID_BASE+41);
pub const CID_COLORFX_CBCR: u32 = (CID_BASE+42);
pub const CID_LASTP1: u32 = (CID_BASE+43);
pub const CID_PRIVATE_BASE: u32 = 0x08000000;

// Control types.
pub const CTRL_TYPE_INTEGER: u32 = 1;
pub const CTRL_TYPE_BOOLEAN: u32 = 2;
pub const CTRL_TYPE_MENU: u32 = 3;
pub const CTRL_TYPE_BUTTON: u32 = 4;
pub const CTRL_TYPE_INTEGER64: u32 = 5;
pub const CTRL_TYPE_STRING: u32 = 7;
pub const CTRL_TYPE_BITMASK: u32 = 8;
pub const CTRL_TYPE_INTEGER_MENU: u32 = 9;

// Control flags.
pub const CTRL_FLAG_DISABLED: u32 = 0x0001;
pub const CTRL_FLAG_GRABBED: u32 = 0x0002;
pub const CTRL_FLAG_READ_ONLY: u32 = 0x0004;
pub const CTRL_FLAG_UPDATE: u32 = 0x0008;
pub const CTRL_FLAG_INACTIVE: u32 = 0x0010;
pub const CTRL_FLAG_SLIDER: u32 = 0x0020;
pub const CTRL_FLAG_WRITE_ONLY: u32 = 0x0040;
pub const CTRL_FLAG_VOLATILE: u32 = 0x0080;
pub const CTRL_FLAG_HAS_PAYLOAD: u32 = 0x0100;
pub const CTRL_FLAG_EXECUTE_ON_WRITE: u32 = 0x0200;

// IOCTL codes.
pub const VIDIOC_ENUM_FMT: usize = 3225441794;
pub const VIDIOC_ENUM_FRAMEINTERVALS: usize = 3224655435;
pub const VIDIOC_ENUM_FRAMESIZES: usize = 3224131146;
pub const VIDIOC_G_CTRL: usize = 3221771803;
pub const VIDIOC_G_EXT_CTRLS: usize = 3223344711;
pub const VIDIOC_QUERYCTRL: usize = 3225703972;
pub const VIDIOC_QUERYMENU: usize = 3224131109;
pub const VIDIOC_REQBUFS: usize = 3222558216;
pub const VIDIOC_S_CTRL: usize = 3221771804;
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
    assert_eq!(mem::size_of::<QueryMenu>(), 44);
    assert_eq!(mem::size_of::<Control>(), 8);
    // assert_eq!(mem::size_of::<ExtControl>(), 20);
}
