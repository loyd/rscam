extern crate rscam;

fn main() {
    let camera = rscam::new("/dev/video0").unwrap();

    for format in camera.formats().iter() {
        println!("{}", format);

        for mode in format.modes.iter() {
            print!("    {}:", mode);

            for fps in mode.fps.iter() {
                print!(" {}", fps);
            }

            println!("");
        }
    }
}
