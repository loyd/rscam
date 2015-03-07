#![feature(core, io, fs)]

extern crate rscam;

use std::fs;
use std::io::Write;
use std::default::Default;

fn main() {
    let mut camera = rscam::new("/dev/video0").unwrap();

    for format in camera.formats().unwrap().iter() {
        println!("{:?}", format);
        println!("  {:?}", camera.resolutions(&format.format).unwrap());
    }

    camera.start(&rscam::Config {
        interval: (1, 10),
        resolution: (1280, 720),
        format: b"MJPG",
        ..Default::default()
    }).unwrap();

    for i in range(0, 10) {
        let frame = camera.capture().unwrap();

        println!("Frame of length {}", frame.data.len());

        let mut file = fs::File::create(&format!("frame-{}.jpg", i)).unwrap();
        file.write_all(frame.data).unwrap();
    }
}
