fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        winresource::WindowsResource::new()
            .compile()
            .expect("Windows metadata resource could not be compiled");
    }
}
