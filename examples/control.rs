extern crate rscam;

use rscam::CID_BRIGHTNESS;
use rscam::{Camera, Control, CtrlData};

fn main() {
    let camera = Camera::new("/dev/video0").unwrap();

    let get_brightness = || match camera.get_control(CID_BRIGHTNESS) {
        Ok(Control {
            data: CtrlData::Integer { value: b, .. },
            ..
        }) => b,
        _ => panic!(),
    };

    let old = get_brightness();

    println!("Current value of brightness: {}", old);
    camera.set_control(CID_BRIGHTNESS, &5).unwrap();
    println!("New value of brightness: {}", get_brightness());

    camera.set_control(CID_BRIGHTNESS, &old).unwrap();
    println!("Restoring old value: {}", get_brightness());
}
