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

for i in range(0u, 10) {
    let frame = camera.capture().unwrap();
    let mut file = fs::File::create(&Path::new(format!("frame-{}.jpg", i)));
    file.write(frame.data).unwrap();
}
```

TODO
----
* `userptr` and `read` methods.
* Control API.
* Checking raspberry pi and x32.
