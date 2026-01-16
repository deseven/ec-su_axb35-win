fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("ec-su_axb35-win.ico");
        res.compile().unwrap();
    }
}
