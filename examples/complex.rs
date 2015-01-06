extern crate rscam;

use std::io::fs;
use std::default::Default;

fn main() {
    let mut camera = rscam::new("/dev/video0").unwrap();

    for format in camera.formats().unwrap().iter() {
        println!("{}", format);
    }

    camera.start(&rscam::Config {
        interval: (1, 10),
        resolution: (1280, 720),
        format: b"MJPG",
        ..Default::default()
    }).unwrap();

    for i in range(0u, 10) {
        let frame = camera.capture().unwrap();

        println!("Frame of length {}", frame.data.len());

        let mut file = fs::File::create(&Path::new(format!("frame-{}.jpg", i)));
        file.write(frame.data).unwrap();
    }
}
