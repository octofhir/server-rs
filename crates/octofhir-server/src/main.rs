use octofhir_server::{init_tracing, shutdown_tracing, ServerBuilder};

#[tokio::main]
async fn main() {
    init_tracing();

    let server = ServerBuilder::new().build();

    if let Err(err) = server.run().await {
        eprintln!("server error: {err}");
    }

    shutdown_tracing();
}
