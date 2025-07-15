use vectorctl_cli::CliError;

#[tokio::main]
async fn main() -> Result<(), CliError> {
    vectorctl_cli::main().await
}
