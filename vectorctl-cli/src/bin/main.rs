use vectorctl_cli::CliError;

#[tokio::main]
async fn main() -> Result<(), Box<CliError>> {
    vectorctl_cli::main().await.map_err(Box::new)
}
