fn main() {
    println!("cargo:rerun-if-changed=../../native/macos/PGAudio");
    #[cfg(target_os = "macos")]
    swift_rs::SwiftLinker::new("14.4")
        .with_package("PGAudio", "../../native/macos/PGAudio")
        .link();
}
