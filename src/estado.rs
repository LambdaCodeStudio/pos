use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct Estado {
    pub pool: PgPool,
    pub jwt_secret: Arc<String>,
}

impl Estado {
    pub fn nuevo(pool: PgPool, jwt_secret: String) -> Self {
        Self {
            pool,
            jwt_secret: Arc::new(jwt_secret),
        }
    }
}
