extern crate rscam;

use std::sync::mpsc;
use std::thread;

use rscam::{Camera, Config};

fn main() {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let mut camera = Camera::new("/dev/video0").unwrap();

        camera
            .start(&Config {
                interval: (1, 10),
                resolution: (1280, 720),
                format: b"MJPG",
                ..Default::default()
            })
            .unwrap();

        for _ in 0..10 {
            let frame = camera.capture().unwrap();
            tx.send(frame).unwrap();
        }
    });

    for i in 0..10 {
        let frame = rx.recv().unwrap();
        println!("Frame #{} of length {}", i, frame.len());
    }
}
