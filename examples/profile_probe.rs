//! Disposable-profile provisioning probe; never point at an established profile.
fn main() {
    let root = std::path::PathBuf::from(std::env::args_os().nth(1).expect("profile root"));
    assert!(!root.exists(), "probe requires a fresh profile");
    let _lock = mesh_tray::identity::lock_profile(&root).unwrap();
    let owner = mesh_tray::identity::ensure(&root).unwrap();
    println!(
        "Provisioned encrypted disposable owner {}",
        owner.owner_id()
    );
}
