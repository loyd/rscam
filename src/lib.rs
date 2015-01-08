#![feature(slicing_syntax)]
#![feature(unsafe_destructor)]

extern crate libc;

use std::{io, fmt, str, error, default, result};
use std::os::unix::Fd;

mod v4l2;


pub type Result<T> = result::Result<T, Error>;

#[derive(Show)]
pub enum Error {
    /// I/O error when using the camera.
    Io(io::IoError),
    /// Unsupported frame interval.
    BadInterval,
    /// Unsupported resolution (width and/or height).
    BadResolution,
    /// Unsupported format of pixel.
    BadFormat,
    /// Unsupported field.
    BadField
}

impl error::FromError<io::IoError> for Error {
    fn from_error(err: io::IoError) -> Error {
        Error::Io(err)
    }
}

/// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/field-order.html#v4l2-field).
#[derive(Copy)]
#[repr(C)]
pub enum Field {
    None = 1,
    Top,
    Bottom,
    Interplaced,
    SeqTB,
    SeqBT,
    Alternate,
    InterplacedTB,
    InterplacedBT
}

#[derive(Copy)]
pub struct Config<'a> {
    /**
     * The mix of numerator and denominator. v4l2 uses frame intervals instead of frame rates.
     * Default is `(1, 10)`.
     */
    pub interval: (u32, u32),
    /**
     * Width and height of frame.
     * Default is `(640, 480)`.
     */
    pub resolution: (u32, u32),
    /**
     * FourCC of format (e.g. `b"RGB3"`). Note that case matters.
     * Default is `b"YUYV"`.
     */
    pub format: &'a [u8],
    /**
     * Storage method of interlaced video.
     * Default is `Field::None` (progressive).
     */
    pub field: Field,
    /**
     * Number of buffers in the queue of camera.
     * Default is `2`.
     */
    pub nbuffers: u32
}

impl<'a> default::Default for Config<'a> {
    fn default() -> Config<'a> {
        Config {
            interval: (1, 10),
            resolution: (640, 480),
            format: b"YUYV",
            field: Field::None,
            nbuffers: 2
        }
    }
}

pub struct FormatInfo {
    /// FourCC of format (e.g. `b"H264"`).
    pub format: [u8; 4],
    /// Information about the format.
    pub description: String,
    /// Raw or compressed.
    pub compressed: bool,
    /// Whether it's transcoded from a different input format.
    pub emulated: bool
}

impl FormatInfo {
    fn new(fourcc: u32, desc: &[u8], flags: u32) -> FormatInfo {
        FormatInfo {
            format: [
                (fourcc >> 0 & 0xff) as u8,
                (fourcc >> 8 & 0xff) as u8,
                (fourcc >> 16 & 0xff) as u8,
                (fourcc >> 24 & 0xff) as u8
            ],

            description: String::from_utf8_lossy(match desc.position_elem(&0) {
                Some(x) => desc.slice_to(x),
                None    => desc
            }).into_owned(),

            compressed: flags & v4l2::FMT_FLAG_COMPRESSED != 0,
            emulated: flags & v4l2::FMT_FLAG_EMULATED != 0
        }
    }

    fn fourcc(fmt: &[u8]) -> u32 {
        fmt[0] as u32 | (fmt[1] as u32) << 8 | (fmt[2] as u32) << 16 | (fmt[3] as u32) << 24
    }
}

impl fmt::Show for FormatInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({}{})", str::from_utf8(self.format.as_slice()).unwrap(),
            self.description, match (self.compressed, self.emulated) {
                (true, true) => ", compressed, emulated",
                (true, false) => ", compressed",
                (false, true) => ", emulated",
                _ => ""
            })
    }
}

pub enum ResolutionInfo {
    Discretes(Vec<(u32, u32)>),
    Stepwise {
        min: (u32, u32),
        max: (u32, u32),
        step: (u32, u32)
    }
}

impl fmt::Show for ResolutionInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ResolutionInfo::Discretes(ref d) => {
                try!(write!(f, "Discretes: {}x{}", d[0].0, d[0].1));

                for res in d.slice_from(1).iter() {
                    try!(write!(f, " , {}x{}", res.0, res.1));
                }

                Ok({})
            },
            ResolutionInfo::Stepwise {min, max, step} =>
                write!(f, "Stepwise from {}x{} to {}x{} by {}x{}",
                    min.0, min.1, max.0, max.1, step.0, step.1)
        }
    }
}

#[derive(Copy)]
pub enum IntervalInfo {
    Discrete(u32, u32)
}

pub struct Frame<'a> {
    /// Slice of one of the buffers.
    pub data: &'a [u8],
    /// Width and height of the frame.
    pub resolution: (u32, u32),
    /// FourCC of the format.
    pub format: [u8; 4],
    fd: Fd,
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
    fd: Fd,
    state: State,
    resolution: (u32, u32),
    format: [u8; 4],
    buffers: Vec<&'a mut [u8]>
}

impl<'a> Camera<'a> {
    pub fn new(device: &str) -> io::IoResult<Camera> {
        Ok(Camera {
            fd: try!(v4l2::open(device)),
            state: State::Idle,
            resolution: (0, 0),
            format: [0; 4],
            buffers: vec![]
        })
    }

    /// Get detailed info about the available formats.
    pub fn formats(&self) -> io::IoResult<Vec<FormatInfo>> {
        let mut formats = vec![];
        let mut fmt = v4l2::FmtDesc::new();

        while try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FMT, &mut fmt)) {
            formats.push(FormatInfo::new(fmt.pixelformat, &fmt.description, fmt.flags));
            fmt.index += 1;
        }

        Ok(formats)
    }

    /// Get detailed info about the available resolutions.
    pub fn resolutions(&self, format: &[u8]) -> Result<ResolutionInfo> {
        if format.len() != 4 {
            return Err(Error::BadFormat);
        }

        let fourcc = FormatInfo::fourcc(format);
        let mut size = v4l2::Frmsizeenum::new(fourcc);

        try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMESIZES, &mut size));

        if fourcc != size.pixelformat {
            return Err(Error::BadFormat);
        }

        if size.ftype == v4l2::FRMSIZE_TYPE_DISCRETE {
            let mut discretes = vec![(size.discrete().width, size.discrete().height)];
            size.index = 1;

            while try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMESIZES, &mut size)) {
                {
                    let discrete = size.discrete();
                    discretes.push((discrete.width, discrete.height));
                }
                size.index += 1;
            }

            Ok(ResolutionInfo::Discretes(discretes))
        } else {
            let sw = size.stepwise();

            Ok(ResolutionInfo::Stepwise {
                min: (sw.min_width, sw.min_height),
                max: (sw.max_width, sw.max_height),
                step: (sw.step_width, sw.step_height)
            })
        }
    }

    /// Get detailed info about the available intervals.
    pub fn intervals(&self, format: &[u8], resolution: (u32, u32)) -> Result<Vec<IntervalInfo>> {
        let mut intervals = vec![];

        if format.len() != 4 {
            return Err(Error::BadFormat);
        }

        let fourcc = FormatInfo::fourcc(format);
        let mut ival = v4l2::Frmivalenum::new(resolution, fourcc);

        while try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMEINTERVALS, &mut ival)) {
            if fourcc != ival.pixelformat {
                return Err(Error::BadFormat);
            }

            if resolution != (ival.width, ival.height) {
                return Err(Error::BadFormat);
            }

            if ival.ftype == v4l2::FRMIVAL_TYPE_DISCRETE {
                intervals.push(
                    IntervalInfo::Discrete(ival.discrete.numerator, ival.discrete.denominator));
            }

            ival.index += 1;
        }

        Ok(intervals)
    }

    /**
     * Start streaming.
     *
     * # Panics
     * if recalled or called after `stop()`.
     */
    pub fn start(&mut self, config: &Config) -> Result<()> {
        assert_eq!(self.state, State::Idle);

        try!(self.tune_format(config.resolution, config.format, config.field));
        try!(self.tune_stream(config.interval));
        try!(self.alloc_buffers(config.nbuffers));

        if let Err(err) = self.streamon() {
            let _ = self.free_buffers();
            return Err(Error::Io(err));
        }

        self.resolution = config.resolution;
        self.format = [config.format[0], config.format[1], config.format[2], config.format[3]];

        self.state = State::Streaming;

        Ok(())
    }

    /**
     * Blocking request of frame.
     * It dequeues buffer from a driver, which will be enqueueed after destructing `Frame`.
     *
     * # Panics
     * If called w/o streaming.
     */
    pub fn capture(&self) -> io::IoResult<Frame> {
        assert_eq!(self.state, State::Streaming);

        let mut buf = v4l2::Buffer::new();

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_DQBUF, &mut buf));
        assert!(buf.index < self.buffers.len() as u32);

        Ok(Frame {
            data: self.buffers[buf.index as usize].slice_to(buf.bytesused as usize),
            resolution: self.resolution,
            format: self.format,
            fd: self.fd,
            buffer: buf
        })
    }

    /**
     * Stop streaming.
     *
     * # Panics
     * If called w/o streaming.
     */
    pub fn stop(&mut self) -> io::IoResult<()> {
        assert_eq!(self.state, State::Streaming);

        try!(self.streamoff());
        try!(self.free_buffers());

        self.state = State::Aborted;

        Ok(())
    }

    fn tune_format(&self, resolution: (u32, u32), format: &[u8], field: Field) -> Result<()> {
        if format.len() != 4 {
            return Err(Error::BadFormat);
        }

        let fourcc = FormatInfo::fourcc(format);
        let mut fmt = v4l2::Format::new(resolution, fourcc, field as u32);

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_S_FMT, &mut fmt));

        if resolution != (fmt.fmt.width, fmt.fmt.height) {
            return Err(Error::BadResolution);
        }

        if fourcc != fmt.fmt.pixelformat {
            return Err(Error::BadFormat);
        }

        if field as u32 != fmt.fmt.field {
            return Err(Error::BadField);
        }

        Ok(())
    }

    fn tune_stream(&self, interval: (u32, u32)) -> Result<()> {
        let mut parm = v4l2::StreamParm::new(interval);

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_S_PARM, &mut parm));
        let time = parm.parm.timeperframe;

        match (time.numerator * interval.1, time.denominator * interval.0) {
            (0, _) | (_, 0) => Err(Error::BadInterval),
            (x, y) if x != y => Err(Error::BadInterval),
            _ => Ok(())
        }
    }

    fn alloc_buffers(&mut self, nbuffers: u32) -> Result<()> {
        let mut req = v4l2::RequestBuffers::new(nbuffers);

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_REQBUFS, &mut req));

        for i in range(0, nbuffers) {
            let mut buf = v4l2::Buffer::new();
            buf.index = i;
            try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_QUERYBUF, &mut buf));

            let region = try!(v4l2::mmap(buf.length as usize, self.fd, buf.m));

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
            let mut buf = v4l2::Buffer::new();
            buf.index = i as u32;

            try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_QBUF, &mut buf));
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
