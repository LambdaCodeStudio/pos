use pos::estado::Estado;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pos=debug,tower_http=info".into()),
        )
        .init();

    let database_url = std::env::var("DATABASE_URL")?;
    let jwt_secret = std::env::var("JWT_SECRET")?;
    let puerto: u16 = std::env::var("PUERTO")
        .unwrap_or_else(|_| "3000".into())
        .parse()?;

    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    pos::identidad::bootstrap::sembrar_admin_si_no_hay_usuarios(&pool).await?;

    let estado = Estado::nuevo(pool, jwt_secret);
    let app = pos::armar_router(estado);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", puerto)).await?;
    tracing::info!("escuchando en puerto {puerto}");
    axum::serve(listener, app).await?;
    Ok(())
}
