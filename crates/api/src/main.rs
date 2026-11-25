use api::AppState;
use aws_config::BehaviorVersion;
use courier::PgCourierService;
use identity::PgIdentityService;

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    let config = aws_config::load_defaults(BehaviorVersion::v2026_01_12()).await;
    let ses_client = aws_sdk_sesv2::client::Client::new(&config);

    let database_url = std::env::var("DATABASE_URL")?;
    let pool = db::connect(&database_url).await?;
    let identity = PgIdentityService::postgres(pool.clone());
    let courier = PgCourierService::postgres(pool, ses_client);

    lambda_http::run(api::router(AppState { identity, courier })).await
}
