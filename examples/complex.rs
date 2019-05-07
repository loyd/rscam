extern crate rscam;

use std::fs;
use std::io::Write;

fn main() {
    let mut camera = rscam::new("/dev/video0").unwrap();

    for wformat in camera.formats() {
        let format = wformat.unwrap();
        println!("{:?}", format);
        println!("  {:?}", camera.resolutions(&format.format).unwrap());
    }

    camera
        .start(&rscam::Config {
            interval: (1, 10),
            resolution: (1280, 720),
            format: b"MJPG",
            ..Default::default()
        })
        .unwrap();

    for i in 0..10 {
        let frame = camera.capture().unwrap();

        println!("Frame of length {}", frame.len());

        let mut file = fs::File::create(&format!("frame-{}.jpg", i)).unwrap();
        file.write_all(&frame[..]).unwrap();
    }
}
