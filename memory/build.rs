fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &[
                "../agent-core/proto/common.proto",
                "../agent-core/proto/memory.proto",
            ],
            &["../agent-core/proto/"],
        )?;
    Ok(())
}
