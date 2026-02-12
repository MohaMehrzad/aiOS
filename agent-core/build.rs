fn main() -> Result<(), Box<dyn std::error::Error>> {
    tonic_build::configure()
        .build_server(true)
        .build_client(true)
        .compile_protos(
            &[
                "proto/common.proto",
                "proto/orchestrator.proto",
                "proto/agent.proto",
                "proto/runtime.proto",
                "proto/tools.proto",
                "proto/memory.proto",
                "proto/api_gateway.proto",
            ],
            &["proto/"],
        )?;
    Ok(())
}
