fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .compile_protos(
            &[
                "../agent-core/proto/common.proto",
                "../agent-core/proto/runtime.proto",
            ],
            &["../agent-core/proto/"],
        )?;
    Ok(())
}
