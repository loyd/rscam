extern crate rscam;

use std::iter;
use std::default::Default;

fn main() {
    let mut camera = rscam::new("/dev/video0").unwrap();

    camera.start(&rscam::Config {
        interval: (1, 10),
        resolution: (1280, 720),
        format: b"YUYV",
        ..Default::default()
    }).unwrap();

    for i in iter::count(1u, 1) {
        let frame = camera.shot().unwrap();
        println!("Frame #{} of length {}", i, frame.data.len());
    }
}
