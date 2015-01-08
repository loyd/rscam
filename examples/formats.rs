extern crate rscam;

fn main() {
    let camera = rscam::new("/dev/video0").unwrap();

    for format in camera.formats().unwrap().iter() {
        println!("{:?}", format);

        println!("    {:?}", camera.resolutions(&format.format).unwrap())
    }
}
