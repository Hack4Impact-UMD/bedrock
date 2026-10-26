use api::AppState;
use identity::PgIdentityService;

#[tokio::main]
async fn main() -> Result<(), lambda_http::Error> {
    let database_url = std::env::var("DATABASE_URL")?;
    let pool = db::connect(&database_url).await?;
    let identity = PgIdentityService::postgres(pool);

    lambda_http::run(api::router(AppState { identity })).await
}
