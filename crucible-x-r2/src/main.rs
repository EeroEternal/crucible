use clap::Parser;
use dotenvy::dotenv;

#[derive(Parser)]
#[command(
    name = "crucible-x-r2",
    about = "Sync X.com bookmarks to Cloudflare R2",
    version
)]
struct Cli {
    /// Numeric X user ID whose bookmarks to sync
    #[arg(short, long)]
    user_id: String,

    /// Only sync the most recent N bookmarks (default: sync all)
    #[arg(short, long)]
    limit: Option<u32>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    let cli = Cli::parse();

    let syncer = crucible_x_r2::CrucibleXSync::new(
        &std::env::var("X_API_KEY").expect("X_API_KEY must be set"),
        &std::env::var("X_API_SECRET").expect("X_API_SECRET must be set"),
        &std::env::var("X_ACCESS_TOKEN").expect("X_ACCESS_TOKEN must be set"),
        &std::env::var("X_ACCESS_SECRET").expect("X_ACCESS_SECRET must be set"),
        &std::env::var("R2_ACCOUNT_ID").expect("R2_ACCOUNT_ID must be set"),
        &std::env::var("R2_ACCESS_KEY_ID").expect("R2_ACCESS_KEY_ID must be set"),
        &std::env::var("R2_SECRET_ACCESS_KEY").expect("R2_SECRET_ACCESS_KEY must be set"),
        &std::env::var("R2_BUCKET_NAME").expect("R2_BUCKET_NAME must be set"),
        &cli.user_id,
    )
    .await?;

    let count = if let Some(limit) = cli.limit {
        syncer.sync_bookmarks_limit(limit).await?
    } else {
        syncer.sync_bookmarks().await?
    };

    println!("✅ Synced {} bookmark(s) to R2", count);
    Ok(())
}
