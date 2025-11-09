fn main() {
    // Disable PDB generation entirely to avoid LNK1318 error
    #[cfg(target_os = "windows")]
    {
        println!("cargo:rustc-link-arg=/DEBUG:NONE");
    }
}