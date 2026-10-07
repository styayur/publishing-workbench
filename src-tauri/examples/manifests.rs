fn main() {
    println!(
        "{}",
        serde_json::to_string_pretty(
            &publishing_workbench::publishing::Registry::builtin().manifests()
        )
        .unwrap()
    );
}
