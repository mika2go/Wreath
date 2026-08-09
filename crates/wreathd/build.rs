fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/wreath.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .set_icon("../../packaging/windows/wreath.ico")
            .set("ProductName", "Wreath")
            .set("FileDescription", "Wreath replay recorder service")
            .compile()
            .expect("failed to embed the Wreath Windows icon");
    }
}
