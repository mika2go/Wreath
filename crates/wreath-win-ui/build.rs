fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/wreath.ico");
    println!("cargo:rerun-if-changed=../../packaging/windows/wreath.manifest");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../packaging/windows/wreath.ico")
            .set_manifest_file("../../packaging/windows/wreath.manifest")
            .set("ProductName", "Wreath")
            .set("FileDescription", "Wreath replay recorder")
            .compile()
            .expect("failed to embed the Wreath Windows icon");
    }
}
