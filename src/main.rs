//! NLA AI Video Creator
//! 
//! A local-first, AI-native Non-Linear Animation editor for generative video production.

mod slint_app;
mod state;
mod ui;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .select()?;

    slint_app::run()?;
    Ok(())
}
