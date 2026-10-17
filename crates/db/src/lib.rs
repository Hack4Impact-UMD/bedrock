use sqlx::postgres::PgPoolOptions;

pub use sqlx::PgPool;

const MAX_CONNECTIONS: u32 = 2;

pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect(database_url)
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    // `#[sqlx::test]` applies `migrations/` to a fresh database, so this fails if any migration is broken.
    #[sqlx::test]
    async fn migrations_apply(_pool: PgPool) {}
}
