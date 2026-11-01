use winisland_plugin_api::packager::PluginPackager;

fn main() {
    let output = PluginPackager::from_cargo()
        .expect("read Cargo.toml")
        .include_dir("runtime")
        .include_dir("models")
        .output(concat!(
            "target/island-voice-",
            env!("CARGO_PKG_VERSION"),
            ".zip"
        ))
        .build()
        .expect("build plugin ZIP");
    println!("plugin package: {}", output.display());
}
