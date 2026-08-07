fn main() {
    println!("cargo:rerun-if-changed=../../packaging/windows/wreath.ico");

    #[cfg(target_os = "windows")]
    winresource::WindowsResource::new()
        .set_icon("../../packaging/windows/wreath.ico")
        .set("ProductName", "Wreath")
        .set("FileDescription", "Wreath replay recorder")
        .compile()
        .expect("failed to embed the Wreath Windows icon");
}
