fn main() {
    let camera = rscam::new("/dev/video0").unwrap();
    println!("{:#?}", camera.capability().unwrap());
}
