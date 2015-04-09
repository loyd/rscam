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

Default feature `use_wrapper` enables the v4l2 wrapper (e.g. `v4l2_ioctl()` instead of `ioctl()`). In this case, there is dependence on *libv4l2*.
