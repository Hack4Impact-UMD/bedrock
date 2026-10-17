// Recompile when migrations change so `#[sqlx::test]` picks up new ones.
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
