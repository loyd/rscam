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
        Err(ref err) if err.kind() == io::ErrorKind::InvalidInput => Ok(false),
        Err(err) => Err(err),
        Ok(_) => Ok(true)
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

pub const BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
pub const FMT_FLAG_COMPRESSED: u32 = 1;
pub const FMT_FLAG_EMULATED: u32 = 2;
pub const FRMIVAL_TYPE_DISCRETE: u32 = 1;
pub const FRMSIZE_TYPE_DISCRETE: u32 = 1;
pub const MEMORY_MMAP: u32 = 1;

// IOCTL codes.
pub const VIDIOC_ENUM_FMT: usize = 3225441794;
pub const VIDIOC_ENUM_FRAMEINTERVALS: usize = 3224655435;
pub const VIDIOC_ENUM_FRAMESIZES: usize = 3224131146;
pub const VIDIOC_REQBUFS: usize = 3222558216;
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
}
