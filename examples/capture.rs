extern crate rscam;

use std::iter;

fn main() {
    let mut camera = rscam::new("/dev/video0").unwrap();

    camera.start(&rscam::Config {
        fps: 10,
        width: 1280,
        height: 720,
        format: b"MJPG"
    }).unwrap();

    for i in iter::count(1u, 1) {
        let frame = camera.shot().unwrap();
        println!("Frame {} with length {}", i, frame.data.len());
    }
}
