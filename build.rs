#[cfg(feature = "static")]
fn main() {
    println!("cargo:rustc-link-lib=static=v4l2");
    println!("cargo:rustc-link-lib=static=v4lconvert");
    println!("cargo:rustc-link-lib=static=jpeg");
}

#[cfg(not(feature = "static"))]
fn main() {}
