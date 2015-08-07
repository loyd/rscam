//! Fast wrapper for v4l2.
//!
//! ```no_run
//! # use std::fs;
//! # use std::io::Write;
//! use rscam::{Camera, Config};
//!
//! let mut camera = Camera::new("/dev/video0").unwrap();
//!
//! camera.start(&Config {
//!     interval: (1, 30),      // 30 fps.
//!     resolution: (1280, 720),
//!     format: b"MJPG",
//!     ..Default::default()
//! }).unwrap();
//!
//! for i in 0..10 {
//!     let frame = camera.capture().unwrap();
//!     let mut file = fs::File::create(&format!("frame-{}.jpg", i)).unwrap();
//!     file.write_all(&frame[..]).unwrap();
//! }
//! ```
//!
//! The wrapper uses v4l2 (e.g. `v4l2_ioctl()` instead of `ioctl()`) until feature `no_wrapper` is
//! enabled. The feature can be useful when it's desirable to avoid dependence on *libv4l2*.

extern crate libc;

mod v4l2;

use std::convert::From;
use std::error;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::os::unix::io::RawFd;
use std::result;
use std::slice;
use std::str;
use std::sync::Arc;

use v4l2::MappedRegion;

/// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/vidioc-queryctrl.html#control-flags)
pub use v4l2::CTRL_FLAG_DISABLED as FLAG_DISABLED;
pub use v4l2::CTRL_FLAG_GRABBED as FLAG_GRABBED;
pub use v4l2::CTRL_FLAG_READ_ONLY as FLAG_READ_ONLY;
pub use v4l2::CTRL_FLAG_UPDATE as FLAG_UPDATE;
pub use v4l2::CTRL_FLAG_INACTIVE as FLAG_INACTIVE;
pub use v4l2::CTRL_FLAG_SLIDER as FLAG_SLIDER;
pub use v4l2::CTRL_FLAG_WRITE_ONLY as FLAG_WRITE_ONLY;
pub use v4l2::CTRL_FLAG_VOLATILE as FLAG_VOLATILE;
pub use v4l2::CTRL_FLAG_HAS_PAYLOAD as FLAG_HAS_PAYLOAD;
pub use v4l2::CTRL_FLAG_EXECUTE_ON_WRITE as FLAG_EXECUTE_ON_WRITE;


pub type Result<T> = result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    /// I/O error when using the camera.
    Io(io::Error),
    /// Unsupported frame interval.
    BadInterval,
    /// Unsupported resolution (width and/or height).
    BadResolution,
    /// Unsupported format of pixels.
    BadFormat,
    /// Unsupported field.
    BadField
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Io(ref err) => write!(f, "I/O error: {}", err),
            Error::BadInterval => write!(f, "Invalid or unsupported frame interval"),
            Error::BadResolution => write!(f, "Invalid or unsupported resolution (width and/or height)"),
            Error::BadFormat => write!(f, "Invalid or unsupported format of pixels"),
            Error::BadField => write!(f, "Invalid or unsupported field")
        }
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        match *self {
            Error::Io(ref err) => err.description(),
            Error::BadInterval => "bad interval",
            Error::BadResolution => "bad resolution",
            Error::BadFormat => "bad format",
            Error::BadField => "bad field"
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        if let Error::Io(ref err) = *self {
            Some(err)
        } else {
            None
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Error {
        Error::Io(err)
    }
}

/// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/field-order.html#v4l2-field).
#[derive(Copy, Clone)]
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

pub struct Config<'a> {
    /// The mix of numerator and denominator. v4l2 uses frame intervals instead of frame rates.
    /// Default is `(1, 10)`.
    pub interval: (u32, u32),
    /// Width and height of frame.
    /// Default is `(640, 480)`.
    pub resolution: (u32, u32),
    /// FourCC of format (e.g. `b"RGB3"`). Note that case matters.
    /// Default is `b"YUYV"`.
    pub format: &'a [u8],
    /// Storage method of interlaced video.
    /// Default is `Field::None` (progressive).
    pub field: Field,
    /// Number of buffers in the queue of camera.
    /// Default is `2`.
    pub nbuffers: u32
}

impl<'a> Default for Config<'a> {
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
            description: buffer_to_string(desc),
            compressed: flags & v4l2::FMT_FLAG_COMPRESSED != 0,
            emulated: flags & v4l2::FMT_FLAG_EMULATED != 0
        }
    }

    fn fourcc(fmt: &[u8]) -> u32 {
        fmt[0] as u32 | (fmt[1] as u32) << 8 | (fmt[2] as u32) << 16 | (fmt[3] as u32) << 24
    }
}

impl fmt::Debug for FormatInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} ({}{})", str::from_utf8(self.format.as_ref()).unwrap(),
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

impl fmt::Debug for ResolutionInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ResolutionInfo::Discretes(ref d) => {
                try!(write!(f, "Discretes: {}x{}", d[0].0, d[0].1));

                for res in (&d[1..]).iter() {
                    try!(write!(f, ", {}x{}", res.0, res.1));
                }

                Ok({})
            },
            ResolutionInfo::Stepwise {min, max, step} =>
                write!(f, "Stepwise from {}x{} to {}x{} by {}x{}",
                    min.0, min.1, max.0, max.1, step.0, step.1)
        }
    }
}

pub enum IntervalInfo {
    Discretes(Vec<(u32, u32)>),
    Stepwise {
        min: (u32, u32),
        max: (u32, u32),
        step: (u32, u32)
    }
}

impl fmt::Debug for IntervalInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            IntervalInfo::Discretes(ref d) => {
                try!(write!(f, "Discretes: {}fps", d[0].1/d[0].0));

                for res in (&d[1..]).iter() {
                    try!(write!(f, ", {}fps", res.1/res.0));
                }

                Ok({})
            },
            IntervalInfo::Stepwise {min, max, step} =>
                write!(f, "Stepwise from {}fps to {}fps by {}fps",
                    max.1/max.0, min.1/min.0, step.1/step.0)
        }
    }
}

pub struct Frame {
    /// Width and height of the frame.
    pub resolution: (u32, u32),
    /// FourCC of the format.
    pub format: [u8; 4],

    region: Arc<MappedRegion>,
    length: u32,
    fd: RawFd,
    buffer: v4l2::Buffer
}

impl Deref for Frame {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.region.ptr, self.length as usize) }
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        let _ = v4l2::xioctl(self.fd, v4l2::VIDIOC_QBUF, &mut self.buffer);
    }
}

#[derive(Debug, PartialEq)]
enum State {
    Idle,
    Streaming,
    Aborted
}

pub struct Camera {
    fd: RawFd,
    state: State,
    resolution: (u32, u32),
    format: [u8; 4],
    buffers: Vec<Arc<MappedRegion>>
}

impl Camera {
    pub fn new(device: &str) -> io::Result<Camera> {
        Ok(Camera {
            fd: try!(v4l2::open(device)),
            state: State::Idle,
            resolution: (0, 0),
            format: [0; 4],
            buffers: vec![]
        })
    }

    /// Get detailed info about the available formats.
    pub fn formats(&self) -> io::Result<Vec<FormatInfo>> {
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
    pub fn intervals(&self, format: &[u8], resolution: (u32, u32)) -> Result<IntervalInfo> {
        if format.len() != 4 {
            return Err(Error::BadFormat);
        }

        let fourcc = FormatInfo::fourcc(format);
        let mut ival = v4l2::Frmivalenum::new(fourcc, resolution);

        try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMEINTERVALS, &mut ival));

        if fourcc != ival.pixelformat {
            return Err(Error::BadFormat);
        }

        if resolution != (ival.width, ival.height) {
            return Err(Error::BadResolution);
        }

        if ival.ftype == v4l2::FRMIVAL_TYPE_DISCRETE {
            let mut discretes = vec![(ival.discrete().numerator, ival.discrete().denominator)];
            ival.index = 1;

            while try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMEINTERVALS, &mut ival)) {
                {
                    let discrete = ival.discrete();
                    discretes.push((discrete.numerator, discrete.denominator));
                }
                ival.index += 1;
            }

            Ok(IntervalInfo::Discretes(discretes))
        } else {
            let sw = ival.stepwise();

            Ok(IntervalInfo::Stepwise {
                min: (sw.min.numerator, sw.min.denominator),
                max: (sw.max.numerator, sw.max.denominator),
                step: (sw.step.numerator, sw.step.denominator)
            })
        }
    }

    /// Get info about the available controls.
    pub fn controls(&self) -> io::Result<Vec<Control>> {
        let mut controls = vec![];

        for id in v4l2::CID_BASE..v4l2::CID_LASTP1 {
            match self.get_control_by_id(id) {
                Ok(ref ctrl) if ctrl.flags & FLAG_DISABLED > 0 => {},
                Ok(ctrl) => controls.push(ctrl),
                Err(ref err) if err.kind() == io::ErrorKind::InvalidInput => {},
                Err(err) => return Err(err)
            }
        }

        for id in v4l2::CID_PRIVATE_BASE.. {
            match self.get_control_by_id(id) {
                Ok(ref ctrl) if ctrl.flags & FLAG_DISABLED > 0 => {},
                Ok(ctrl) => controls.push(ctrl),
                Err(ref err) if err.kind() == io::ErrorKind::InvalidInput => break,
                Err(err) => return Err(err)
            }
        }

        Ok(controls)
    }

    /// Get info about the control by id.
    pub fn get_control(&self, cid: CID) -> io::Result<Control> {
        self.get_control_by_id(cid.into())
    }

    fn get_control_by_id(&self, id: u32) -> io::Result<Control> {
        let mut qctrl = v4l2::QueryCtrl::new();
        qctrl.id = id;
        let mut ctrl = v4l2::Control::new();
        ctrl.id = id;

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_QUERYCTRL, &mut qctrl));
        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_G_CTRL, &mut ctrl));

        let data = match qctrl.qtype {
            v4l2::CTRL_TYPE_INTEGER => CtrlData::Integer {
                value: ctrl.value,
                default: qctrl.default_value,
                minimum: qctrl.minimum,
                maximum: qctrl.maximum,
                step: qctrl.step
            },
            v4l2::CTRL_TYPE_BOOLEAN => CtrlData::Boolean {
                value: ctrl.value != 0,
                default: qctrl.default_value != 0
            },
            v4l2::CTRL_TYPE_MENU => CtrlData::Menu {
                value: ctrl.value as u32,
                default: qctrl.default_value as u32,
                items: {
                    let mut items = vec![];
                    let mut qmenu = v4l2::QueryMenu::new();
                    qmenu.id = id;
                    for index in (qctrl.minimum..qctrl.maximum+1) {
                        qmenu.index = index as u32;
                        if try!(v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_QUERYMENU, &mut qmenu)) {
                            items.push(CtrlMenuItem {
                                index: index as u32,
                                name: buffer_to_string(&qmenu.name)
                            });
                        }
                    }
                    items
                }
            },
            v4l2::CTRL_TYPE_BUTTON => CtrlData::Button,
            _ => CtrlData::Unknown
        };

        Ok(Control {
            id: id,
            name: buffer_to_string(&qctrl.name),
            data: data,
            flags: qctrl.flags
        })
    }

    /// Set value of the control.
    pub fn set_control(&self, ctrl: Ctrl) -> Result<()> {
        let (id, val) = ctrl.into();
        let mut ctrl = v4l2::Control::new();
        ctrl.id = id;
        ctrl.value = val;
        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_S_CTRL, &mut ctrl));
        Ok(())
    }

    /// Start streaming.
    ///
    /// # Panics
    /// If recalled or called after `stop()`.
    pub fn start(&mut self, config: &Config) -> Result<()> {
        assert_eq!(self.state, State::Idle);

        try!(self.tune_format(config.resolution, config.format, config.field));
        try!(self.tune_stream(config.interval));
        try!(self.alloc_buffers(config.nbuffers));

        if let Err(err) = self.streamon() {
            self.free_buffers();
            return Err(Error::Io(err));
        }

        self.resolution = config.resolution;
        self.format = [config.format[0], config.format[1], config.format[2], config.format[3]];

        self.state = State::Streaming;

        Ok(())
    }

    /// Blocking request of frame.
    /// It dequeues buffer from a driver, which will be enqueueed after destructing `Frame`.
    ///
    /// # Panics
    /// If called w/o streaming.
    pub fn capture(&self) -> io::Result<Frame> {
        assert_eq!(self.state, State::Streaming);

        let mut buf = v4l2::Buffer::new();

        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_DQBUF, &mut buf));
        assert!(buf.index < self.buffers.len() as u32);

        Ok(Frame {
            resolution: self.resolution,
            format: self.format,
            region: self.buffers[buf.index as usize].clone(),
            length: buf.bytesused,
            fd: self.fd,
            buffer: buf
        })
    }

    /// Stop streaming. Otherwise it's called after destructing `Camera`.
    ///
    /// # Panics
    /// If called w/o streaming.
    pub fn stop(&mut self) -> io::Result<()> {
        assert_eq!(self.state, State::Streaming);

        try!(self.streamoff());
        self.free_buffers();

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

        for i in 0..nbuffers {
            let mut buf = v4l2::Buffer::new();
            buf.index = i;
            try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_QUERYBUF, &mut buf));

            let region = try!(v4l2::mmap(buf.length as usize, self.fd, buf.m));
            self.buffers.push(Arc::new(region));
        }

        Ok(())
    }

    fn free_buffers(&mut self) {
        self.buffers.clear();
    }

    fn streamon(&self) -> io::Result<()> {
        for i in 0..self.buffers.len() {
            let mut buf = v4l2::Buffer::new();
            buf.index = i as u32;

            try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_QBUF, &mut buf));
        }

        let mut typ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_STREAMON, &mut typ));

        Ok(())
    }

    fn streamoff(&mut self) -> io::Result<()> {
        let mut typ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        try!(v4l2::xioctl(self.fd, v4l2::VIDIOC_STREAMOFF, &mut typ));

        Ok(())
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        if self.state == State::Streaming {
            let _ = self.stop();
        }

        let _ = v4l2::close(self.fd);
    }
}

pub enum CID {
    Brightness, Contrast, Saturation, Hue, AudioVolume, AudioBalance, AudioBass, AudioTreble,
    AudioMute, AudioLoudness, BlackLevel, AutoWhiteBalance, DoWhiteBalance, RedBalance,
    BlueBalance, Gamma, Whiteness, Exposure, Autogain, Gain, Hflip, Vflip, PowerLineFrequency,
    HueAuto, WhiteBalanceTemperature, Sharpness, BacklightCompensation, ChromaAgc, ColorKiller,
    Colorfx, Autobrightness, BandStopFilter, Rotate, BgColor, ChromaGain, Illuminators1,
    Illuminators2, MinBuffersForCapture, MinBuffersForOutput, AlphaComponent, ColorfxCbcr,
    Custom(u32)
}

impl Into<u32> for CID {
    fn into(self) -> u32 {
        v4l2::CID_BASE + match self {
            CID::Brightness => 0,
            CID::Contrast => 1,
            CID::Saturation => 2,
            CID::Hue => 3,
            CID::AudioVolume => 5,
            CID::AudioBalance => 6,
            CID::AudioBass => 7,
            CID::AudioTreble => 8,
            CID::AudioMute => 9,
            CID::AudioLoudness => 10,
            CID::BlackLevel => 11,
            CID::AutoWhiteBalance => 12,
            CID::DoWhiteBalance => 13,
            CID::RedBalance => 14,
            CID::BlueBalance => 15,
            CID::Gamma => 16,
            CID::Whiteness => 16,
            CID::Exposure => 17,
            CID::Autogain => 18,
            CID::Gain => 19,
            CID::Hflip => 20,
            CID::Vflip => 21,
            CID::PowerLineFrequency => 24,
            CID::HueAuto => 25,
            CID::WhiteBalanceTemperature => 26,
            CID::Sharpness => 27,
            CID::BacklightCompensation => 28,
            CID::ChromaAgc => 29,
            CID::ColorKiller => 30,
            CID::Colorfx => 31,
            CID::Autobrightness => 32,
            CID::BandStopFilter => 33,
            CID::Rotate => 34,
            CID::BgColor => 35,
            CID::ChromaGain => 36,
            CID::Illuminators1 => 37,
            CID::Illuminators2 => 38,
            CID::MinBuffersForCapture => 39,
            CID::MinBuffersForOutput => 40,
            CID::AlphaComponent => 41,
            CID::ColorfxCbcr => 42,
            CID::Custom(id) => return id
        }
    }
}

pub enum Ctrl {
    Brightness(i32),
    Contrast(i32),
    Saturation(i32),
    Hue(i32),
    AudioVolume(i32),
    AudioBalance(i32),
    AudioBass(i32),
    AudioTreble(i32),
    AudioMute(bool),
    AudioLoudness(bool),
    BlackLevel(i32),
    AutoWhiteBalance(bool),
    DoWhiteBalance,
    RedBalance(i32),
    BlueBalance(i32),
    Gamma(i32),
    Whiteness(i32),
    Exposure(i32),
    Autogain(bool),
    Gain(i32),
    Hflip(bool),
    Vflip(bool),
    PowerLineFrequencyDisabled,
    PowerLineFrequency50hz,
    PowerLineFrequency60hz,
    PowerLineFrequencyAuto,
    HueAuto(bool),
    WhiteBalanceTemperature(i32),
    Sharpness(i32),
    BacklightCompensation(i32),
    ChromaAgc(bool),
    ColorKiller(bool),
    ColorfxNone,
    ColorfxBw,
    ColorfxSepia,
    ColorfxNegative,
    ColorfxEmboss,
    ColorfxSketch,
    ColorfxSkyBlue,
    ColorfxGrassGreen,
    ColorfxSkinWhiten,
    ColorfxVivid,
    ColorfxAqua,
    ColorfxArtFreeze,
    ColorfxSilhouette,
    ColorfxSolarization,
    ColorfxAntique,
    ColorfxSetCbcr,
    Autobrightness(bool),
    BandStopFilter(i32),
    Rotate(i32),
    BgColor(i32),
    ChromaGain(i32),
    Illuminators1(bool),
    Illuminators2(bool),
    MinBuffersForCapture(i32),
    MinBuffersForOutput(i32),
    AlphaComponent(i32),
    ColorfxCbcr(i32)
}

impl Into<(u32, i32)> for Ctrl {
    fn into(self) -> (u32, i32) {
        use Ctrl::*;
        let (cid, val) = match self {
            Brightness(b) => (CID::Brightness, b),
            Contrast(c) => (CID::Contrast, c),
            Saturation(s) => (CID::Saturation, s),
            Hue(h) => (CID::Hue, h),
            AudioVolume(v) => (CID::AudioVolume, v),
            AudioBalance(b) => (CID::AudioBalance, b),
            AudioBass(b) => (CID::AudioBass, b),
            AudioTreble(t) => (CID::AudioTreble, t),
            AudioMute(m) => (CID::AudioMute, m as i32),
            AudioLoudness(l) => (CID::AudioLoudness, l as i32) ,
            BlackLevel(l) => (CID::BlackLevel, l),
            AutoWhiteBalance(b) => (CID::AutoWhiteBalance, b as i32),
            DoWhiteBalance => (CID::DoWhiteBalance, 0),
            RedBalance(b) => (CID::RedBalance, b),
            BlueBalance(b) => (CID::BlueBalance, b),
            Gamma(g) => (CID::Gamma, g),
            Whiteness(w) => (CID::Whiteness, w),
            Exposure(e) => (CID::Exposure, e),
            Autogain(a) => (CID::Brightness, a as i32),
            Gain(g) => (CID::Gain, g),
            Hflip(f) => (CID::Hflip, f as i32),
            Vflip(f) => (CID::Vflip, f as i32),
            PowerLineFrequencyDisabled => (CID::PowerLineFrequency, 0),
            PowerLineFrequency50hz => (CID::PowerLineFrequency, 1),
            PowerLineFrequency60hz => (CID::PowerLineFrequency, 2),
            PowerLineFrequencyAuto => (CID::PowerLineFrequency, 3),
            HueAuto(h) => (CID::HueAuto, h as i32),
            WhiteBalanceTemperature(t) => (CID::WhiteBalanceTemperature, t),
            Sharpness(s) => (CID::Sharpness, s),
            BacklightCompensation(c) => (CID::BacklightCompensation, c),
            ChromaAgc(a) => (CID::ChromaAgc, a as i32),
            ColorKiller(k) => (CID::ColorKiller, k as i32),
            ColorfxNone => (CID::Colorfx, 0),
            ColorfxBw => (CID::Colorfx, 1),
            ColorfxSepia => (CID::Colorfx, 2),
            ColorfxNegative => (CID::Colorfx, 3),
            ColorfxEmboss => (CID::Colorfx, 4),
            ColorfxSketch => (CID::Colorfx, 5),
            ColorfxSkyBlue => (CID::Colorfx, 6),
            ColorfxGrassGreen => (CID::Colorfx, 7),
            ColorfxSkinWhiten => (CID::Colorfx, 8),
            ColorfxVivid => (CID::Colorfx, 9),
            ColorfxAqua => (CID::Colorfx, 10),
            ColorfxArtFreeze => (CID::Colorfx, 11),
            ColorfxSilhouette => (CID::Colorfx, 12),
            ColorfxSolarization => (CID::Colorfx, 13),
            ColorfxAntique => (CID::Colorfx, 14),
            ColorfxSetCbcr => (CID::Colorfx, 15),
            Autobrightness(a) => (CID::Autobrightness, a as i32),
            BandStopFilter(f) => (CID::BandStopFilter, f),
            Rotate(r) => (CID::Rotate, r),
            BgColor(c) => (CID::BgColor, c),
            ChromaGain(g) => (CID::ChromaGain, g),
            Illuminators1(i) => (CID::Illuminators1, i as i32),
            Illuminators2(i) => (CID::Illuminators2, i as i32),
            MinBuffersForCapture(b) => (CID::MinBuffersForCapture, b),
            MinBuffersForOutput(b) => (CID::MinBuffersForOutput, b),
            AlphaComponent(a) => (CID::AlphaComponent, a),
            ColorfxCbcr(c) => (CID::ColorfxCbcr, c)
        };

        (cid.into(), val)
    }
}

pub struct Control {
    pub id: u32,
    pub name: String,
    pub data: CtrlData,
    pub flags: u32
}

pub enum CtrlData {
    Integer {
        value: i32,
        default: i32,
        minimum: i32,
        maximum: i32,
        step: i32
    },
    Boolean {
        value: bool,
        default: bool
    },
    Menu {
        value: u32,
        default: u32,
        items: Vec<CtrlMenuItem>
    },
    IntegerMenu {
        value: u32,
        default: u32,
        items: Vec<CtrlIntMenuItem>
    },
    Bitmask {
        value: u32,
        default: u32,
        maximum: u32
    },
    Integer64 {
        value: i64,
        default: i64,
        minimum: i64,
        maximum: i64,
        step: i64
    },
    String {
        value: String,
        minimum: u32,
        maximum: u32,
        step: u32
    },
    Button,
    Unknown
}

pub struct CtrlMenuItem {
    pub index: u32,
    pub name: String
}

pub struct CtrlIntMenuItem {
    pub index: u32,
    pub value: i64
}

fn buffer_to_string(buf: &[u8]) -> String {
    // Instead of unstable `position_elem()`.
    String::from_utf8_lossy(match buf.iter().position(|&c| c == 0) {
        Some(x) => &buf[..x],
        None    => buf
    }).into_owned()
}

/// Alias for `Camera::new()`.
pub fn new(device: &str) -> io::Result<Camera> {
    Camera::new(device)
}
