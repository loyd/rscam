use std::time::Duration;

#[cfg(feature = "tokio_async")]
#[tokio::main]
async fn main() {
    loop {
        std::thread::sleep(Duration::from_secs(1));

        let mut camera = match rscam::new("/dev/video0") {
            Ok(camera) => camera,
            Err(e) => {
                eprintln!("failed to open camera: {}", e);
                continue;
            }
        };

        let res = camera.start(&rscam::Config {
            interval: (1, 30),
            resolution: (1920, 1080),
            format: b"MJPG",
            ..Default::default()
        });

        if let Err(e) = res {
            eprintln!("failed to start camera: {}", e);
            continue;
        }

        for i in 1.. {
            let frame = match camera.capture().await {
                Ok(frame) => frame,
                Err(e) => {
                    eprintln!("failed to capture frame: {}", e);
                    break;
                }
            };
            println!("Frame #{} of length {}", i, frame.len());
        }
    }
}
