use cli::CliError;

#[tokio::main]
async fn main() -> Result<(), CliError> {
    cli::main().await
}
