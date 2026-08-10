use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    let proto_dir = manifest_dir.join("../../proto");
    let proto_file = proto_dir.join("clusterscope.proto");

    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .out_dir(&out_dir)
        .compile(&[proto_file], &[proto_dir])?;

    println!("cargo:rerun-if-changed=../../proto/clusterscope.proto");
    Ok(())
}
