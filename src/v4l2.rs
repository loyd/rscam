use std::{io, os, raw, mem};

// C types and constants.
use libc::{c_void, c_char, c_int, c_ulong, size_t, EINTR};
use libc::types::os::arch::posix88::{off_t};
use libc::types::os::common::posix01::timeval as Timeval;
use libc::consts::os::posix88::{O_RDWR, PROT_READ, PROT_WRITE, MAP_SHARED};

use std::c_str::ToCStr;


#[link(name="v4l2")]
extern {
    pub fn v4l2_open(file: *const c_char, flags: c_int, arg: c_int) -> c_int;
    pub fn v4l2_close(fd: c_int) -> c_int;
    pub fn v4l2_ioctl(fd: c_int, request: c_ulong, argp: *mut c_void) -> c_int;
    pub fn v4l2_mmap(start: *mut c_void, length: size_t, prot: c_int,
                 flags: c_int, fd: c_int, offset: off_t) -> *mut c_void;
    pub fn v4l2_munmap(start: *mut c_void, length: size_t) -> c_int;
}

macro_rules! check(
    ($cond:expr) =>
        (try!(match !$cond && os::errno() > 0 {
            true  => Err(io::IoError::last_error()),
            false => Ok(())
        }))
);

pub fn open(file: &str) -> io::IoResult<int> {
    let c_str = file.to_c_str();
    let fd = unsafe { v4l2_open(c_str.as_ptr(), O_RDWR, 0) as int };
    check!(fd != -1);
    Ok(fd)
}

pub fn close(fd: int) -> io::IoResult<()> {
    check!(unsafe { v4l2_close(fd as c_int) != -1 });
    Ok(())
}

pub fn xioctl<T>(fd: int, request: uint, arg: &mut T) -> io::IoResult<()> {
    let argp: *mut T = arg;

    check!(unsafe {
        let mut ok;

        loop {
            ok = v4l2_ioctl(fd as c_int, request as c_ulong, argp as *mut c_void) != -1;
            if ok || os::errno() != EINTR as uint {
                break;
            }
        }

        ok
    });

    Ok(())
}

pub fn xioctl_valid<T>(fd: int, request: uint, arg: &mut T) -> io::IoResult<bool> {
    match xioctl(fd, request, arg) {
        Err(io::IoError { kind: io::InvalidInput, .. }) => Ok(false),
        Err(err) => Err(err),
        Ok(_) => Ok(true)
    }
}

pub fn mmap<'a>(length: uint, fd: int, offset: uint) -> io::IoResult<&'a mut [u8]> {
    let ptr = unsafe { v4l2_mmap(0 as *mut c_void, length as size_t, PROT_READ|PROT_WRITE,
                                 MAP_SHARED, fd as c_int, offset as off_t) as *mut u8 };

    check!(ptr != -1 as *mut u8);
    Ok(unsafe { mem::transmute(raw::Slice { data: ptr, len: length}) })
}

pub fn munmap(region: &mut [u8]) -> io::IoResult<()> {
    check!(unsafe {
        v4l2_munmap(&mut region[0] as *mut u8 as *mut c_void, region.len() as size_t) == 0
    });

    Ok(())
}

#[repr(C)]
pub struct Format {
    pub ftype: u32,
    #[cfg(target_word_size="64")]
    padding: u32,
    pub fmt: PixFormat,
    space: [u8; 156]
}

impl Format {
    #[cfg(target_word_size="64")]
    pub fn new(resolution: (u32, u32), fourcc: u32, field: u32) -> Format {
        Format {
            ftype: BUF_TYPE_VIDEO_CAPTURE,
            padding: 0,
            fmt: PixFormat::new(resolution, fourcc, field),
            space: [0; 156]
        }
    }

    #[cfg(target_word_size="32")]
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
        PixFormat {
            width: resolution.0,
            height: resolution.1,
            pixelformat: fourcc,
            field: field,
            bytesperline: 0,
            sizeimage: 0,
            colorspace: 0,
            private: 0,
            flags: 0,
            ycbcr_enc: 0,
            quantization: 0
        }
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
    pub m: uint,   // offset (__u32) or userptr (ulong)
    pub length: u32,
    pub input: u32,
    reserved: u32
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer {
            index: 0,
            btype: BUF_TYPE_VIDEO_CAPTURE,
            bytesused: 0,
            flags: 0,
            field: 0,
            timestamp: Timeval {
                tv_sec: 0,
                tv_usec: 0
            },
            timecode: TimeCode {
                ttype: 0,
                flags: 0,
                frames: 0,
                seconds: 0,
                minutes: 0,
                hours: 0,
                userbits: [0; 4]
            },
            sequence: 0,
            memory: MEMORY_MMAP,
            m: 0,
            length: 0,
            input: 0,
            reserved: 0
        }
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
        FmtDesc {
            index: 0,
            ftype: BUF_TYPE_VIDEO_CAPTURE,
            flags: 0,
            description: [0; 32],
            pixelformat: 0,
            reserved: [0; 4]
        }
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
        StreamParm {
            ptype: BUF_TYPE_VIDEO_CAPTURE,
            parm: CaptureParm {
                capability: 0,
                capturemode: 0,
                timeperframe: Fract {
                    numerator: interval.0,
                    denominator: interval.1
                },
                extendedmode: 0,
                readbuffers: 0,
                reserved: [0; 4]
            },
            space: [0; 160]
        }
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
    pub discrete: FrmsizeDiscrete,
    reserved: [u32; 6]
}

impl Frmsizeenum {
    pub fn new() -> Frmsizeenum {
        Frmsizeenum {
            index: 0,
            pixelformat: 0,
            ftype: 0,
            discrete: FrmsizeDiscrete {
                width: 0,
                height: 0
            },
            reserved: [0; 6]
        }
    }
}

#[repr(C)]
pub struct FrmsizeDiscrete {
    pub width: u32,
    pub height: u32
}

#[repr(C)]
pub struct Frmivalenum {
    pub index: u32,
    pub pixelformat: u32,
    pub width: u32,
    pub height: u32,
    pub ftype: u32,
    pub discrete: Fract,
    reserved: [u32; 6]
}

impl Frmivalenum {
    pub fn new() -> Frmivalenum {
        Frmivalenum {
            index: 0,
            pixelformat: 0,
            width: 0,
            height: 0,
            ftype: 0,
            discrete: Fract {
                numerator: 0,
                denominator: 0
            },
            reserved: [0; 6]
        }
    }
}

pub static BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
pub static FMT_FLAG_COMPRESSED: u32 = 1;
pub static FMT_FLAG_EMULATED: u32 = 2;
pub static FRMIVAL_TYPE_DISCRET: u32 = 1;
pub static FRMSIZE_TYPE_DISCRETE: u32 = 1;
pub static MEMORY_MMAP: u32 = 1;

// IOCTL codes.
pub static VIDIOC_ENUM_FMT: uint = 3225441794;
pub static VIDIOC_ENUM_FRAMEINTERVALS: uint = 3224655435;
pub static VIDIOC_ENUM_FRAMESIZES: uint = 3224131146;
pub static VIDIOC_REQBUFS: uint = 3222558216;
pub static VIDIOC_S_PARM: uint = 3234616854;
pub static VIDIOC_STREAMOFF: uint = 1074026003;
pub static VIDIOC_STREAMON: uint = 1074026002;

#[cfg(target_word_size = "64")]
pub static VIDIOC_DQBUF: uint = 3227014673;
#[cfg(target_word_size = "32")]
pub static VIDIOC_DQBUF: uint = 3225703953;

#[cfg(target_word_size = "64")]
pub static VIDIOC_QBUF: uint = 3227014671;
#[cfg(target_word_size = "32")]
pub static VIDIOC_QBUF: uint = 3225703951;

#[cfg(target_word_size = "64")]
pub static VIDIOC_QUERYBUF: uint = 3227014665;
#[cfg(target_word_size = "32")]
pub static VIDIOC_QUERYBUF: uint = 3225703945;

#[cfg(target_word_size = "64")]
pub static VIDIOC_S_FMT: uint = 3234878981;
#[cfg(target_word_size = "32")]
pub static VIDIOC_S_FMT: uint = 3234616837;

#[test]
fn test_sizes() {
    if cfg!(target_word_size = "64") {
        assert_eq!(mem::size_of::<Format>(), 208);
    } else {
        assert_eq!(mem::size_of::<Format>(), 204);
    }

    if cfg!(target_word_size = "64") {
        assert_eq!(mem::size_of::<Buffer>(), 88);
    } else {
        assert_eq!(mem::size_of::<Buffer>(), 68);
    }

    assert_eq!(mem::size_of::<StreamParm>(), 204);
    assert_eq!(mem::size_of::<FmtDesc>(), 64);
    assert_eq!(mem::size_of::<Frmsizeenum>(), 44);
    assert_eq!(mem::size_of::<Frmivalenum>(), 52);
}
