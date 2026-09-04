fn main() {
    println!("cargo:rerun-if-changed=native");
    println!("cargo:rerun-if-changed=../driver/Shared.h");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        cc::Build::new()
            .cpp(true)
            .file("native/gpu.cpp")
            .flag("/std:c++17")
            .flag("/EHsc")
            .flag("/W4")
            .define("NOMINMAX", None)
            .compile("sidecardos_gpu");
        for lib in [
            "d3d11", "dxgi", "mfplat", "mf", "mfuuid", "ole32", "oleaut32", "advapi32",
        ] {
            println!("cargo:rustc-link-lib={lib}");
        }
    }
}
