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

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
compile_error!("rscam (v4l2) is for linux/freebsd only");

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

pub use consts::*;
pub use v4l2::pubconsts as consts;

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
    BadField,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Io(ref err) => write!(f, "I/O error: {}", err),
            Error::BadInterval => write!(f, "Invalid or unsupported frame interval"),
            Error::BadResolution => {
                write!(f, "Invalid or unsupported resolution (width and/or height)")
            }
            Error::BadFormat => write!(f, "Invalid or unsupported format of pixels"),
            Error::BadField => write!(f, "Invalid or unsupported field"),
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
            Error::BadField => "bad field",
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
    /// Storage method of interlaced video. See `FIELD_*` constants.
    /// [Details](http://linuxtv.org/downloads/v4l-dvb-apis/field-order.html#v4l2-field).
    /// Default is `FIELD_NONE` (progressive).
    pub field: u32,
    /// Number of buffers in the queue of camera.
    /// Default is `2`.
    pub nbuffers: u32,
}

impl<'a> Default for Config<'a> {
    fn default() -> Config<'a> {
        Config {
            interval: (1, 10),
            resolution: (640, 480),
            format: b"YUYV",
            field: FIELD_NONE,
            nbuffers: 2,
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
    pub emulated: bool,
}

impl FormatInfo {
    fn new(fourcc: u32, desc: &[u8], flags: u32) -> FormatInfo {
        FormatInfo {
            format: [
                (fourcc & 0xff) as u8,
                (fourcc >> 8 & 0xff) as u8,
                (fourcc >> 16 & 0xff) as u8,
                (fourcc >> 24 & 0xff) as u8,
            ],
            description: buffer_to_string(desc),
            compressed: flags & v4l2::FMT_FLAG_COMPRESSED != 0,
            emulated: flags & v4l2::FMT_FLAG_EMULATED != 0,
        }
    }

    fn fourcc(fmt: &[u8]) -> u32 {
        u32::from(fmt[0])
            | (u32::from(fmt[1])) << 8
            | (u32::from(fmt[2])) << 16
            | (u32::from(fmt[3])) << 24
    }
}

impl fmt::Debug for FormatInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let format = str::from_utf8(self.format.as_ref()).unwrap();

        let flags = match (self.compressed, self.emulated) {
            (true, true) => ", compressed, emulated",
            (true, false) => ", compressed",
            (false, true) => ", emulated",
            _ => "",
        };

        write!(f, "{} ({}{})", format, self.description, flags)
    }
}

pub enum ResolutionInfo {
    Discretes(Vec<(u32, u32)>),
    Stepwise {
        min: (u32, u32),
        max: (u32, u32),
        step: (u32, u32),
    },
}

impl fmt::Debug for ResolutionInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            ResolutionInfo::Discretes(ref d) => {
                write!(f, "Discretes: {}x{}", d[0].0, d[0].1)?;

                for res in (&d[1..]).iter() {
                    write!(f, ", {}x{}", res.0, res.1)?;
                }

                Ok(())
            }
            ResolutionInfo::Stepwise { min, max, step } => write!(
                f,
                "Stepwise from {}x{} to {}x{} by {}x{}",
                min.0, min.1, max.0, max.1, step.0, step.1
            ),
        }
    }
}

pub enum IntervalInfo {
    Discretes(Vec<(u32, u32)>),
    Stepwise {
        min: (u32, u32),
        max: (u32, u32),
        step: (u32, u32),
    },
}

impl fmt::Debug for IntervalInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            IntervalInfo::Discretes(ref d) => {
                write!(f, "Discretes: {}fps", d[0].1 / d[0].0)?;

                for res in (&d[1..]).iter() {
                    write!(f, ", {}fps", res.1 / res.0)?;
                }

                Ok(())
            }
            IntervalInfo::Stepwise { min, max, step } => write!(
                f,
                "Stepwise from {}fps to {}fps by {}fps",
                max.1 / max.0,
                min.1 / min.0,
                step.1 / step.0
            ),
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
    buffer: v4l2::Buffer,
}

impl Frame {
    /// Return frame timestamp in microseconds using monotonically
    /// nondecreasing clock
    pub fn get_timestamp(&self) -> u64 {
        let t = self.buffer.timestamp;
        1_000_000 * (t.tv_sec as u64) + (t.tv_usec as u64)
    }
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
    Aborted,
}

pub struct Camera {
    fd: RawFd,
    state: State,
    resolution: (u32, u32),
    format: [u8; 4],
    buffers: Vec<Arc<MappedRegion>>,
}

impl Camera {
    pub fn new(device: &str) -> io::Result<Camera> {
        Ok(Camera {
            fd: v4l2::open(device)?,
            state: State::Idle,
            resolution: (0, 0),
            format: [0; 4],
            buffers: vec![],
        })
    }

    /// Get detailed info about the available formats.
    pub fn formats(&self) -> FormatIter {
        FormatIter {
            camera: self,
            index: 0,
        }
    }

    /// Get detailed info about the available resolutions.
    pub fn resolutions(&self, format: &[u8]) -> Result<ResolutionInfo> {
        if format.len() != 4 {
            return Err(Error::BadFormat);
        }

        let fourcc = FormatInfo::fourcc(format);
        let mut size = v4l2::Frmsizeenum::new(fourcc);

        v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMESIZES, &mut size)?;

        if fourcc != size.pixelformat {
            return Err(Error::BadFormat);
        }

        if size.ftype == v4l2::FRMSIZE_TYPE_DISCRETE {
            let mut discretes = vec![(size.discrete().width, size.discrete().height)];
            size.index = 1;

            while v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMESIZES, &mut size)? {
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
                step: (sw.step_width, sw.step_height),
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

        v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMEINTERVALS, &mut ival)?;

        if fourcc != ival.pixelformat {
            return Err(Error::BadFormat);
        }

        if resolution != (ival.width, ival.height) {
            return Err(Error::BadResolution);
        }

        if ival.ftype == v4l2::FRMIVAL_TYPE_DISCRETE {
            let mut discretes = vec![(ival.discrete().numerator, ival.discrete().denominator)];
            ival.index = 1;

            while v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_ENUM_FRAMEINTERVALS, &mut ival)? {
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
                step: (sw.step.numerator, sw.step.denominator),
            })
        }
    }

    /// Get info about all controls.
    pub fn controls(&self) -> ControlIter {
        ControlIter {
            camera: self,
            id: 0,
            class: 0,
        }
    }

    /// Get info about available controls by class (see `CLASS_*` constants).
    pub fn controls_by_class(&self, class: u32) -> ControlIter {
        ControlIter {
            camera: self,
            id: class as u32,
            class,
        }
    }

    /// Get info about the control by id.
    pub fn get_control(&self, id: u32) -> io::Result<Control> {
        let mut qctrl = v4l2::QueryCtrl::new(id);
        v4l2::xioctl(self.fd, v4l2::VIDIOC_QUERYCTRL, &mut qctrl)?;

        let data = match qctrl.qtype {
            v4l2::CTRL_TYPE_INTEGER => CtrlData::Integer {
                value: self.get_control_value(qctrl.id)?,
                default: qctrl.default_value,
                minimum: qctrl.minimum,
                maximum: qctrl.maximum,
                step: qctrl.step,
            },
            v4l2::CTRL_TYPE_BOOLEAN => CtrlData::Boolean {
                value: self.get_control_value(qctrl.id)? != 0,
                default: qctrl.default_value != 0,
            },
            v4l2::CTRL_TYPE_MENU => CtrlData::Menu {
                value: self.get_control_value(qctrl.id)? as u32,
                default: qctrl.default_value as u32,
                items: self.get_menu_items(qctrl.id, qctrl.minimum as u32, qctrl.maximum as u32)?,
            },
            v4l2::CTRL_TYPE_BUTTON => CtrlData::Button,
            v4l2::CTRL_TYPE_INTEGER64 => {
                let mut qectrl = v4l2::QueryExtCtrl::new(qctrl.id);

                v4l2::xioctl(self.fd, v4l2::VIDIOC_QUERY_EXT_CTRL, &mut qectrl)?;

                CtrlData::Integer64 {
                    value: self.get_ext_control_value(qctrl.id)?,
                    default: qectrl.default_value,
                    minimum: qectrl.minimum,
                    maximum: qectrl.maximum,
                    step: qectrl.step as i64,
                }
            }
            v4l2::CTRL_TYPE_CTRL_CLASS => CtrlData::CtrlClass,
            v4l2::CTRL_TYPE_STRING => CtrlData::String {
                value: self.get_string_control(qctrl.id, qctrl.maximum as u32)?,
                minimum: qctrl.minimum as u32,
                maximum: qctrl.maximum as u32,
                step: qctrl.step as u32,
            },
            v4l2::CTRL_TYPE_BITMASK => CtrlData::Bitmask {
                value: self.get_control_value(qctrl.id)? as u32,
                default: qctrl.default_value as u32,
                maximum: qctrl.maximum as u32,
            },
            v4l2::CTRL_TYPE_INTEGER_MENU => CtrlData::IntegerMenu {
                value: self.get_control_value(qctrl.id)? as u32,
                default: qctrl.default_value as u32,
                items: self.get_int_menu_items(
                    qctrl.id,
                    qctrl.minimum as u32,
                    qctrl.maximum as u32,
                )?,
            },
            _ => CtrlData::Unknown,
        };

        Ok(Control {
            id: qctrl.id,
            name: buffer_to_string(&qctrl.name),
            data,
            flags: qctrl.flags,
        })
    }

    fn get_control_value(&self, id: u32) -> io::Result<i32> {
        let mut ctrl = v4l2::Control::new(id);
        v4l2::xioctl(self.fd, v4l2::VIDIOC_G_CTRL, &mut ctrl)?;
        Ok(ctrl.value)
    }

    fn get_ext_control_value(&self, id: u32) -> io::Result<i64> {
        let mut ctrl = v4l2::ExtControl::new(id, 0);
        {
            let mut ctrls = v4l2::ExtControls::new(id & v4l2::ID2CLASS, &mut ctrl);
            v4l2::xioctl(self.fd, v4l2::VIDIOC_G_EXT_CTRLS, &mut ctrls)?;
        }
        Ok(ctrl.value)
    }

    fn get_menu_items(&self, id: u32, min: u32, max: u32) -> io::Result<Vec<CtrlMenuItem>> {
        let mut items = vec![];
        let mut qmenu = v4l2::QueryMenu::new(id);

        for index in min..=max {
            qmenu.index = index as u32;

            if v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_QUERYMENU, &mut qmenu)? {
                items.push(CtrlMenuItem {
                    index,
                    name: buffer_to_string(qmenu.data.name()),
                });
            }
        }

        Ok(items)
    }

    fn get_int_menu_items(&self, id: u32, min: u32, max: u32) -> io::Result<Vec<CtrlIntMenuItem>> {
        let mut items = vec![];
        let mut qmenu = v4l2::QueryMenu::new(id);

        for index in min..=max {
            qmenu.index = index as u32;

            if v4l2::xioctl_valid(self.fd, v4l2::VIDIOC_QUERYMENU, &mut qmenu)? {
                items.push(CtrlIntMenuItem {
                    index,
                    value: qmenu.data.value(),
                });
            }
        }

        Ok(items)
    }

    fn get_string_control(&self, id: u32, size: u32) -> io::Result<String> {
        let mut buffer = Vec::with_capacity(size as usize + 1);
        let mut ctrl = v4l2::ExtControl::new(id, size + 1);
        ctrl.value = buffer.as_mut_ptr() as i64;
        let mut ctrls = v4l2::ExtControls::new(id & v4l2::ID2CLASS, &mut ctrl);
        v4l2::xioctl(self.fd, v4l2::VIDIOC_G_EXT_CTRLS, &mut ctrls)?;
        unsafe { buffer.set_len(size as usize + 1) };
        Ok(buffer_to_string(&buffer[..]))
    }

    /// Set value of the control.
    pub fn set_control<T: Settable>(&self, id: u32, value: &T) -> io::Result<()> {
        let mut ctrl = v4l2::ExtControl::new(id, 0);
        ctrl.value = value.unify();
        let mut ctrls = v4l2::ExtControls::new(id & v4l2::ID2CLASS, &mut ctrl);
        v4l2::xioctl(self.fd, v4l2::VIDIOC_S_EXT_CTRLS, &mut ctrls)?;
        Ok(())
    }

    /// Start streaming.
    ///
    /// # Panics
    /// If recalled or called after `stop()`.
    pub fn start(&mut self, config: &Config) -> Result<()> {
        assert_eq!(self.state, State::Idle);

        self.tune_format(config.resolution, config.format, config.field)?;
        self.tune_stream(config.interval)?;
        self.alloc_buffers(config.nbuffers)?;

        if let Err(err) = self.streamon() {
            self.free_buffers();
            return Err(Error::Io(err));
        }

        self.resolution = config.resolution;
        self.format = [
            config.format[0],
            config.format[1],
            config.format[2],
            config.format[3],
        ];

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

        v4l2::xioctl(self.fd, v4l2::VIDIOC_DQBUF, &mut buf)?;
        assert!(buf.index < self.buffers.len() as u32);

        Ok(Frame {
            resolution: self.resolution,
            format: self.format,
            region: self.buffers[buf.index as usize].clone(),
            length: buf.bytesused,
            fd: self.fd,
            buffer: buf,
        })
    }

    /// Stop streaming. Otherwise it's called after destructing `Camera`.
    ///
    /// # Panics
    /// If called w/o streaming.
    pub fn stop(&mut self) -> io::Result<()> {
        assert_eq!(self.state, State::Streaming);

        self.streamoff()?;
        self.free_buffers();

        self.state = State::Aborted;

        Ok(())
    }

    fn tune_format(&self, resolution: (u32, u32), format: &[u8], field: u32) -> Result<()> {
        if format.len() != 4 {
            return Err(Error::BadFormat);
        }

        let fourcc = FormatInfo::fourcc(format);
        let mut fmt = v4l2::Format::new(resolution, fourcc, field as u32);

        v4l2::xioctl(self.fd, v4l2::VIDIOC_S_FMT, &mut fmt)?;

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

        v4l2::xioctl(self.fd, v4l2::VIDIOC_S_PARM, &mut parm)?;
        let time = parm.parm.timeperframe;

        match (time.numerator * interval.1, time.denominator * interval.0) {
            (0, _) | (_, 0) => Err(Error::BadInterval),
            (x, y) if x != y => Err(Error::BadInterval),
            _ => Ok(()),
        }
    }

    fn alloc_buffers(&mut self, nbuffers: u32) -> Result<()> {
        let mut req = v4l2::RequestBuffers::new(nbuffers);

        v4l2::xioctl(self.fd, v4l2::VIDIOC_REQBUFS, &mut req)?;

        for i in 0..nbuffers {
            let mut buf = v4l2::Buffer::new();
            buf.index = i;
            v4l2::xioctl(self.fd, v4l2::VIDIOC_QUERYBUF, &mut buf)?;

            let region = v4l2::mmap(buf.length as usize, self.fd, buf.m)?;
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

            v4l2::xioctl(self.fd, v4l2::VIDIOC_QBUF, &mut buf)?;
        }

        let mut typ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        v4l2::xioctl(self.fd, v4l2::VIDIOC_STREAMON, &mut typ)?;

        Ok(())
    }

    fn streamoff(&mut self) -> io::Result<()> {
        let mut typ = v4l2::BUF_TYPE_VIDEO_CAPTURE;
        v4l2::xioctl(self.fd, v4l2::VIDIOC_STREAMOFF, &mut typ)?;

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

pub struct FormatIter<'a> {
    camera: &'a Camera,
    index: u32,
}

impl<'a> Iterator for FormatIter<'a> {
    type Item = io::Result<FormatInfo>;

    fn next(&mut self) -> Option<io::Result<FormatInfo>> {
        let mut fmt = v4l2::FmtDesc::new();
        fmt.index = self.index;

        match v4l2::xioctl_valid(self.camera.fd, v4l2::VIDIOC_ENUM_FMT, &mut fmt) {
            Ok(true) => {
                self.index += 1;
                Some(Ok(FormatInfo::new(
                    fmt.pixelformat,
                    &fmt.description,
                    fmt.flags,
                )))
            }
            Ok(false) => None,
            Err(err) => Some(Err(err)),
        }
    }
}

pub struct ControlIter<'a> {
    camera: &'a Camera,
    id: u32,
    class: u32,
}

impl<'a> Iterator for ControlIter<'a> {
    type Item = io::Result<Control>;

    fn next(&mut self) -> Option<io::Result<Control>> {
        match self.camera.get_control(self.id | v4l2::NEXT_CTRL) {
            Ok(ref ctrl) if self.class > 0 && ctrl.id & v4l2::ID2CLASS != self.class as u32 => None,
            Err(ref err) if err.kind() == io::ErrorKind::InvalidInput => None,
            Ok(ctrl) => {
                self.id = ctrl.id;
                Some(Ok(ctrl))
            }
            err @ Err(_) => Some(err),
        }
    }
}

pub trait Settable {
    fn unify(&self) -> i64;
}

impl Settable for i64 {
    fn unify(&self) -> i64 {
        *self
    }
}

impl Settable for i32 {
    fn unify(&self) -> i64 {
        i64::from(*self)
    }
}

impl Settable for u32 {
    fn unify(&self) -> i64 {
        i64::from(*self)
    }
}

impl Settable for bool {
    fn unify(&self) -> i64 {
        *self as i64
    }
}

impl<'a> Settable for &'a str {
    fn unify(&self) -> i64 {
        self.as_ptr() as i64
    }
}

impl Settable for String {
    fn unify(&self) -> i64 {
        self.as_ptr() as i64
    }
}

pub struct Control {
    pub id: u32,
    pub name: String,
    pub data: CtrlData,
    /// See `FLAG_*` constants for details.
    pub flags: u32,
}

pub enum CtrlData {
    Integer {
        value: i32,
        default: i32,
        minimum: i32,
        maximum: i32,
        step: i32,
    },
    Boolean {
        value: bool,
        default: bool,
    },
    Menu {
        value: u32,
        default: u32,
        items: Vec<CtrlMenuItem>,
    },
    Button,
    Integer64 {
        value: i64,
        default: i64,
        minimum: i64,
        maximum: i64,
        step: i64,
    },
    CtrlClass,
    String {
        value: String,
        minimum: u32,
        maximum: u32,
        step: u32,
    },
    Bitmask {
        value: u32,
        default: u32,
        maximum: u32,
    },
    IntegerMenu {
        value: u32,
        default: u32,
        items: Vec<CtrlIntMenuItem>,
    },
    Unknown,
}

pub struct CtrlMenuItem {
    pub index: u32,
    pub name: String,
}

pub struct CtrlIntMenuItem {
    pub index: u32,
    pub value: i64,
}

fn buffer_to_string(buf: &[u8]) -> String {
    // Instead of unstable `position_elem()`.
    String::from_utf8_lossy(match buf.iter().position(|&c| c == 0) {
        Some(x) => &buf[..x],
        None => buf,
    }).into_owned()
}

/// Alias for `Camera::new()`.
pub fn new(device: &str) -> io::Result<Camera> {
    Camera::new(device)
}
