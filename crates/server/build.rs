fn main() {
    // sqlx::migrate! embeds the migrations but cannot tell cargo to watch them on stable
    println!("cargo:rerun-if-changed=migrations");
}
