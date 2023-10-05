# rscam

[![Build status](https://travis-ci.org/loyd/rscam.svg)](https://travis-ci.org/loyd/rscam)
[![Crate info](https://img.shields.io/crates/v/rscam.svg)](https://crates.io/crates/rscam)
[![Documentation](https://docs.rs/rscam/badge.svg)](https://docs.rs/rscam)

## This project is no longer maintained
Consider to use [https://github.com/raymanfx/libv4l-rs](libv4l-rs) or something else.
If you would be interested in taking over some of the maintenance of the project, please let me know.

## Overview

Rust wrapper for v4l2.

```rust
let mut camera = rscam::new("/dev/video0").unwrap();

camera.start(&rscam::Config {
    interval: (1, 30),      // 30 fps.
    resolution: (1280, 720),
    format: b"MJPG",
    ..Default::default()
}).unwrap();

for i in 0..10 {
    let frame = camera.capture().unwrap();
    let mut file = fs::File::create(&format!("frame-{}.jpg", i)).unwrap();
    file.write_all(&frame[..]).unwrap();
}
```

The wrapper uses v4l2 (e.g. `v4l2_ioctl()` instead of `ioctl()`) until feature `no_wrapper` is enabled. The feature can be useful when it's desirable to avoid dependence on *libv4l2* (for example, cross-compilation).

## License

Licensed under either of

* Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
