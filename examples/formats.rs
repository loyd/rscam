extern crate rscam;

use rscam::{Camera, ResolutionInfo};

fn main() {
    let camera = Camera::new("/dev/video0").unwrap();

    for wformat in camera.formats() {
        let format = wformat.unwrap();
        println!("{:?}", format);

        let resolutions = camera.resolutions(&format.format).unwrap();

        if let ResolutionInfo::Discretes(d) = resolutions {
            for resol in &d {
                println!(
                    "  {}x{}  {:?}",
                    resol.0,
                    resol.1,
                    camera.intervals(&format.format, *resol).unwrap()
                );
            }
        } else {
            println!("  {:?}", resolutions);
        }
    }
}
