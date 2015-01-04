#![feature(globs)]
#![feature(macro_rules)]
#![feature(slicing_syntax)]
#![feature(unsafe_destructor)]

extern crate libc;

use std::{io, fmt, str, error};

mod v4l2;


#[derive(Show)]
pub enum Error {
    Io(io::IoError),
    /// Unsupported resolution (width and/or height).
    BadResolution,
    /// Unsupported format of pixel.
    BadFormat,
    /// Unsupported frame interval.
    BadInterval
}

impl error::FromError<io::IoError> for Error {
    fn from_error(err: io::IoError) -> Error {
        Error::Io(err)
    }
}

#[derive(Copy)]
pub struct Config<'a> {
    /// The mix of numerator and denominator. v4l2 uses frame intervals instead of frame rates.
    pub interval: (u32, u32),
    pub width: u32,
    pub height: u32,
    /// Note that case matters.
    pub format: &'a [u8]
}

pub struct FormatInfo {
    pub format: [u8; 4],
    pub desc: String,
    pub compressed: bool,
    /// Whether it's transcoded from a different input format.
    pub emulated: bool,
    /// Resolutions and intervals for the format.
    pub modes: Vec<ModeInfo>
}

impl FormatInfo {
    fn new(fourcc: u32, desc: &[u8; 32], flags: u32) -> FormatInfo {
        FormatInfo {
            format: [
                (fourcc >> 0 & 0xff) as u8,
                (fourcc >> 8 & 0xff) as u8,
                (fourcc >> 16 & 0xff) as u8,
                (fourcc >> 24 & 0xff) as u8
            ],

            desc: unsafe {
                String::from_raw_buf(desc.as_ptr())
            },

            compressed: flags & v4l2::FMT_FLAG_COMPRESSED != 0,
            emulated: flags & v4l2::FMT_FLAG_EMULATED != 0,

            modes: vec![]
        }
    }

    fn fourcc(val: &[u8]) -> u32 {
        assert_eq!(val.len(), 4);
        val[0] as u32 | (val[1] as u32) << 8 | (val[2] as u32) << 16 | (val[3] as u32) << 24
    }
}

impl fmt::Show for FormatInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({}{})", str::from_utf8(self.format.as_slice()).unwrap(),
            self.desc, match (self.compressed, self.emulated) {
                (true, true) => ", compressed, emulated",
                (true, false) => ", compressed",
                (false, true) => ", emulated",
                _ => ""
            })
    }
}

pub struct ModeInfo {
    pub width: u32,
    pub height: u32,
    pub intervals: Vec<(u32, u32)>
}

impl ModeInfo {
    pub fn new(width: u32, height: u32) -> ModeInfo {
        ModeInfo {
            width: width,
            height: height,
            intervals: vec![]
        }
    }
}

impl fmt::Show for ModeInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}x{}", self.width, self.height)
    }
}

pub struct Frame<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
    fd: int,
    buffer: v4l2::Buffer
}

#[unsafe_destructor]
impl<'a> Drop for Frame<'a> {
    #[allow(unused_must_use)]
    fn drop(&mut self) {
        v4l2::xioctl(self.fd, v4l2::VIDIOC_QBUF, &mut self.buffer);
    }
}

#[derive(Show, PartialEq)]
enum State {
    Idle,
    Streaming,
    Aborted
}

pub struct Camera<'a> {
    state: State,
    fd: int,
    interval: (u32, u32),
    width: u32,
    height: u32,
    fourcc: u32,
    buffers: Vec<&'a mut [u8]>,
    nbuffers: u32
}

impl<'a> Camera<'a> {
    /// Construct with the default number of buffers.
    pub fn new(device: &str) -> io::IoResult<Camera> {
        Camera::with_nbuffers(device, 4)
    }

    /// Construct with the specified number of buffers.
    pub fn with_nbuffers(device: &str, nbuffers: u32) -> io::IoResult<Camera> {
        Ok(Camera {
            state: State::Idle,
            fd: try!(v4l2::open(device)),
            interval: (0, 0),
            width: 0,
            height: 0,
            fourcc: 0,
            buffers: Vec::with_capacity(nbuffers as uint),
            nbuffers: nbuffers
        })
    }

    /// Get detailed info about the available formats.
    pub fn formats(&self) -> io::IoResult<Vec<FormatInfo>> {
        let mut res = vec![];
        let mut fmt = v4l2::FmtDesc::new();
        let mut size = v4l2::Frmsizeenum::new();
        let mut ival = v4l2::Frmivalenum::new();

        // Get formats.
        while try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FMT, &mut fmt)) {
            let mut format = FormatInfo::new(fmt.pixelformat, &fmt.description, fmt.flags);

            size.index = 0;
            size.pixelformat = fmt.pixelformat;
            ival.pixelformat = fmt.pixelformat;

            // Get modes.
            while try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMESIZES, &mut size)) {
                if size.ftype != v4l2::FRMSIZE_TYPE_DISCRETE {
                    size.index += 1;
                    continue;
                }

                let mut mode = ModeInfo::new(size.discrete.width, size.discrete.height);

                ival.index = 0;
                ival.width = mode.width;
                ival.height = mode.height;

                // Get intervals.
                while try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMEINTERVALS,
                                              &mut ival)) {
                    if ival.ftype == v4l2::FRMIVAL_TYPE_DISCRET {
                        mode.intervals.push((ival.discrete.numerator, ival.discrete.denominator));
                    }

                    ival.index += 1;
                }

                format.modes.push(mode);
                size.index += 1;
            }

            res.push(format);
            fmt.index += 1;
        }

        Ok(res)
    }

    /// Start streaming.
    pub fn start(&mut self, config: &Config) -> Result<(), Error> {
        assert_eq!(self.state, State::Idle);

        self.interval = config.interval;
        self.width = config.width;
        self.height = config.height;
        self.fourcc = FormatInfo::fourcc(config.format);

        try!(self.tune_format());
        try!(self.tune_stream());
        try!(self.alloc_buffers());

        if let Err(err) = self.streamon() {
            let _ = self.free_buffers();
            return Err(Error::Io(err));
        }

        self.state = State::Streaming;

        Ok(())
    }

    /**
     * Blocking request of frame.
     * It dequeues buffer from a driver, which will be enqueueed after destructing `Frame`.
     */
    pub fn shot(&self) -> io::IoResult<Frame> {
        assert_eq!(self.state, State::Streaming);

        let mut buffer = v4l2::Buffer::new();

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_DQBUF, &mut buffer));
        assert!(buffer.index < self.buffers.len() as u32);

        Ok(Frame {
            data: self.buffers[buffer.index as uint][0..buffer.bytesused as uint],
            width: self.width,
            height: self.height,
            fd: self.fd,
            buffer: buffer
        })
    }

    /// Stop streaming.
    pub fn stop(&mut self) -> io::IoResult<()> {
        assert_eq!(self.state, State::Streaming);

        try!(self.streamoff());
        try!(self.free_buffers());

        self.state = State::Aborted;

        Ok(())
    }

    fn tune_format(&self) -> Result<(), Error> {
        let mut format = v4l2::Format::new(self.width, self.height, self.fourcc);

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_S_FMT, &mut format));

        if self.width != format.fmt.width || self.height != format.fmt.height {
            return Err(Error::BadResolution);
        }

        if self.fourcc != format.fmt.pixelformat {
            return Err(Error::BadFormat);
        }

        Ok(())
    }

    fn tune_stream(&self) -> Result<(), Error> {
        let mut parm = v4l2::StreamParm::new(self.interval);

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_S_PARM, &mut parm));

        let time = parm.parm.timeperframe;
        assert!(time.denominator != 0);

        if time.numerator * self.interval.1 != time.denominator * self.interval.0 {
            return Err(Error::BadInterval);
        }

        Ok(())
    }

    fn alloc_buffers(&mut self) -> io::IoResult<()> {
        let mut req = v4l2::RequestBuffers::new(self.nbuffers);

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_REQBUFS, &mut req));

        for i in range(0, self.nbuffers) {
            let mut buffer = v4l2::Buffer::new();
            buffer.index = i;
            try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_QUERYBUF, &mut buffer));

            let region = try!(v4l2::mmap(buffer.length as uint, self.fd, buffer.m));

            self.buffers.push(region);
        }

        Ok(())
    }

    fn free_buffers(&mut self) -> io::IoResult<()> {
        let mut res = Ok(());

        for buffer in self.buffers.iter_mut() {
            if let (&Ok(_), Err(err)) = (&res, v4l2::munmap(*buffer)) {
                res = Err(err);
            }
        }

        self.buffers.clear();
        res
    }

    fn streamon(&self) -> io::IoResult<()> {
        for i in range(0, self.buffers.len()) {
            let mut buffer = v4l2::Buffer::new();
            buffer.index = i as u32;

            try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_QBUF, &mut buffer));
        }

        let mut typ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_STREAMON, &mut typ));

        Ok(())
    }

    fn streamoff(&mut self) -> io::IoResult<()> {
        let mut typ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_STREAMOFF, &mut typ));

        Ok(())
    }
}

#[unsafe_destructor]
impl<'a> Drop for Camera<'a> {
    #[allow(unused_must_use)]
    fn drop(&mut self) {
        if self.state == State::Streaming {
            self.stop();
        }

        v4l2::close(self.fd);
    }
}

/// Alias for `Camera::new()`.
pub fn new(device: &str) -> io::IoResult<Camera> {
    Camera::new(device)
}
