fn main() {
    let dir = std::env::var("PFFFT_SRC").unwrap_or_else(|_| "../../.local/pffft".into());
    cc::Build::new().file(format!("{dir}/pffft.c")).file(format!("{dir}/fftpack.c")).include(&dir).opt_level(3).warnings(false).compile("pffft");
    println!("cargo:rerun-if-changed={dir}/pffft.c");
}
