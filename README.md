rscam
=====

Rust wrapper for v4l2.

* [Documentation](http://loyd.github.io/rscam)
* [Crate info](https://crates.io/crates/rscam)

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
