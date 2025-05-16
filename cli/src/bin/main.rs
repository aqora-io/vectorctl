use qdrant_tools_cli::CliError;

#[tokio::main]
async fn main() -> Result<(), CliError> {
    qdrant_tools_cli::main().await
}
