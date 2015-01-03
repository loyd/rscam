extern crate rscam;

use std::io::fs;

fn main() {
    let mut camera = rscam::new("/dev/video0").unwrap();

    for format in camera.formats().iter() {
        println!("{}", format);
    }

    camera.start(&rscam::Config {
        interval: (1, 10),
        width: 1280,
        height: 720,
        format: b"MJPG"
    }).unwrap();

    for i in range(0u, 10) {
        let frame = camera.shot().unwrap();

        println!("Frame of length {}", frame.data.len());

        let mut file = fs::File::create(&Path::new(format!("frame-{}.jpg", i)));
        file.write(frame.data).unwrap();
    }
}
