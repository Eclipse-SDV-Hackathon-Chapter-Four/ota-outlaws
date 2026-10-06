fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut includes = vec!["proto"];
    if std::path::Path::new("/usr/include/google/protobuf/timestamp.proto").exists() {
        includes.push("/usr/include");
    }

    tonic_build::configure()
        .build_server(false)
        .compile(
            &["proto/kuksa/val/v1/val.proto"],
            &includes,
        )?;
    Ok(())
}
