// Copyright 2024-2026 Reflective Labs
// SPDX-License-Identifier: MIT

//! Build script for protobuf code generation.
//!
//! Compiles `proto/kernel.proto` when the `server` or `grpc-client` feature is enabled.

fn main() {
    #[cfg(feature = "server")]
    {
        tonic_build::configure()
            .build_server(true)
            .build_client(true)
            .out_dir("src/server/generated")
            .compile(
                &[concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../schema/proto/kernel.proto"
                )],
                &[concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/proto")],
            )
            .expect("Failed to compile kernel.proto");
    }

    #[cfg(all(feature = "grpc-client", not(feature = "server")))]
    {
        tonic_build::configure()
            .build_server(false)
            .build_client(true)
            .out_dir("src/server/generated")
            .compile(
                &[concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../../schema/proto/kernel.proto"
                )],
                &[concat!(env!("CARGO_MANIFEST_DIR"), "/../../schema/proto")],
            )
            .expect("Failed to compile kernel.proto");
    }
}
