extern crate rscam;

fn main() {
    let camera = rscam::new("/dev/video0").unwrap();

    for format in camera.formats().unwrap().iter() {
        println!("{:?}", format);

        for mode in camera.resolutions(format.format).unwrap().iter() {
            print!("    {:?}:", mode);

            // for interval in camera.intervals(format.format).unwrap().iter() {
            //     print!(" {:?}", interval);
            // }

            println!("");
        }
    }
}
