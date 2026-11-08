fn main() {
    // Only checks that the SDK compiles and links on this machine.
    let _ = std::any::type_name::<spacetimedb_sdk::Identity>();
    println!("spacetimedb-sdk 2.10.1 linked");
}
