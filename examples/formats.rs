extern crate rscam;

fn main() {
    let camera = rscam::new("/dev/video0").unwrap();

    for format in camera.formats().unwrap().iter() {
        println!("{:?}", format);

        let resolutions = camera.resolutions(&format.format).unwrap();

        if let rscam::ResolutionInfo::Discretes(d) = resolutions {
            for resol in d.iter() {
                println!("  {}x{}  {:?}", resol.0, resol.1,
                    camera.intervals(&format.format, *resol).unwrap());
            }
        } else {
            println!("  {:?}", resolutions);
        }
    }
}
