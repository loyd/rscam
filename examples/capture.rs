extern crate rscam;

use std::iter;

fn main() {
    let mut camera = rscam::new("/dev/video0").unwrap();

    camera.start(&rscam::Config {
        interval: (1, 10),
        width: 1280,
        height: 720,
        format: b"YUYV"
    }).unwrap();

    for i in iter::count(1u, 1) {
        let frame = camera.shot().unwrap();
        println!("Frame #{} of length {}", i, frame.data.len());
    }
}
