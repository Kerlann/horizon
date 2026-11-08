fn main() {
    let root =
        std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!("cargo:rerun-if-changed=../horizon-mod-api/src");
    println!("cargo:rerun-if-changed=../horizon-mod-api-gen/src");
    println!("cargo:rerun-if-changed=../../mod-api");
    let code = std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(move || horizon_mod_api_gen::generate_wire_host(&root))
        .expect("starting host generator")
        .join()
        .expect("host generator panicked")
        .expect("generating host boundary");
    std::fs::write(out.join("wire_host_generated.rs"), code).expect("writing host boundary");
}
