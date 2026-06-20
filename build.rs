fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default() == "riscv32" {
        println!("cargo:rustc-link-arg=-Tlinkall.x");
    }
}
