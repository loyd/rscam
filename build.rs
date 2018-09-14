fn main() {
    if ! cfg!(target_os = "linux") || cfg!(target_os = "freebsd") {
        println!("rscam (v4l2) is for linux/freebsd only");
        std::process::exit(1);
    }
}
