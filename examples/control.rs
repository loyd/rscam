extern crate rscam;

use rscam::{Camera, CID, Ctrl, Control, CtrlData};


fn main() {
    let camera = Camera::new("/dev/video0").unwrap();

    let get_brightness = ||
        match camera.get_control(CID::Brightness) {
            Ok(Control { data: CtrlData::Integer { value: b, .. }, .. }) => b,
            _ => panic!()
        };

    let old = get_brightness();

    println!("Current value of brightness: {}", old);
    camera.set_control(Ctrl::Brightness(5)).unwrap();
    println!("New value of brightness: {}", get_brightness());

    camera.set_control(Ctrl::Brightness(old)).unwrap();
    println!("Restoring old value: {}", get_brightness());
}
