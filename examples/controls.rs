extern crate rscam;

use rscam::FLAG_DISABLED;
use rscam::{Camera, CtrlData};

fn main() {
    let camera = Camera::new("/dev/video0").unwrap();

    for wctrl in camera.controls() {
        let ctrl = wctrl.unwrap();

        if let CtrlData::CtrlClass = ctrl.data {
            println!("\n[{}]\n", ctrl.name);
            continue;
        }

        print!("{:>32} ", ctrl.name);

        if ctrl.flags & FLAG_DISABLED != 0 {
            println!("(disabled)");
            continue;
        }

        match ctrl.data {
            CtrlData::Integer {
                value,
                default,
                minimum,
                maximum,
                step,
            } => println!(
                "(int)     min={} max={} step={} default={} value={}",
                minimum, maximum, step, default, value
            ),
            CtrlData::Boolean { value, default } => {
                println!("(bool)    default={} value={}", default, value)
            }
            CtrlData::Menu {
                value,
                default,
                ref items,
                ..
            } => {
                println!("(menu)    default={} value={}", default, value);
                for item in items {
                    println!("{:42} {}: {}", "", item.index, item.name);
                }
            }
            CtrlData::IntegerMenu {
                value,
                default,
                ref items,
                ..
            } => {
                println!("(intmenu) default={} value={}", default, value);
                for item in items {
                    println!(
                        "{:42} {}: {} ({:#x})",
                        "", item.index, item.value, item.value
                    );
                }
            }
            CtrlData::Bitmask {
                value,
                default,
                maximum,
            } => println!(
                "(bitmask) max={:x} default={:x} value={:x}",
                maximum, default, value
            ),
            CtrlData::Integer64 {
                value,
                default,
                minimum,
                maximum,
                step,
            } => println!(
                "(int64)   min={} max={} step={} default={} value={}",
                minimum, maximum, step, default, value
            ),
            CtrlData::String {
                ref value,
                minimum,
                maximum,
                step,
            } => println!(
                "(str)     min={} max={} step={} value={}",
                minimum, maximum, step, value
            ),
            CtrlData::Button => println!("(button)"),
            _ => {}
        }
    }
}
