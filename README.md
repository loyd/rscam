rscam
=====

Rust wrapper for v4l2.

* [Documentation](http://loyd.github.io/rscam)
* [Crate info](https://crates.io/crates/rscam)

```rust
let mut camera = rscam::new("/dev/video0").unwrap();

camera.start(&rscam::Config {
    interval: (1, 30),  // 30 fps.
    width: 1280,
    height: 720,
    format: b"MJPG"
}).unwrap();

for i in range(0u, 10) {
    let frame = camera.shot().unwrap();
    let mut file = fs::File::create(&Path::new(format!("frame-{}.jpg", i)));
    file.write(frame.data).unwrap();
}
```

TODO
----
* `userptr` and `read` methods.
* test on raspberry pi.
