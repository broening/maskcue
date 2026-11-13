fn main() {
    // Signalsmith Stretch (Pitch + Formant) als C++ mitbauen.
    println!("cargo::rerun-if-changed=cpp");
    cc::Build::new()
        .cpp(true)
        .std("c++14")
        .file("cpp/stretch_bruecke.cpp")
        .include("cpp")
        .opt_level(3)
        .compile("maskcue_stretch");

    tauri_build::build()
}
