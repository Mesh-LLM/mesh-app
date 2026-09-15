//! Print this machine's Mesh owner identity, creating it if the profile is new.
//! Points at a profile directory you pass in; never at somebody's live one
//! unless that is exactly what you mean to read.
fn main() {
    let root = std::path::PathBuf::from(std::env::args_os().nth(1).expect("mesh profile root"));
    let owner = mesh_tray::identity::establish(&root).unwrap();
    println!("Mesh owner in {}: {owner}", root.display());
}
