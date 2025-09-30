use vectorctl::{Cli, Parser, VectorTrait};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let dataset_url = cli.database_url;
    let api_key = cli.api_key;

    let context = vectorctl::Context::new(vectorctl::Backend::new(&dataset_url, api_key)?);

    vectorctl::run_migrate(qdrant_exemple::Migrator, &context)
        .await
        .map_err(|err| anyhow::anyhow!(err.to_string()))
}
