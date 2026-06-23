#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=assets/app-icon.ico");
    let mut resource = winresource::WindowsResource::new();
    resource.set_icon("assets/app-icon.ico");
    resource
        .compile()
        .expect("failed to embed LatentSlate Windows icon resource");
}

#[cfg(not(windows))]
fn main() {}
