use std::sync::Arc;

use sqlx::PgPool;

use crate::config::Config;
use crate::journey::storage::R2;
use crate::journey::JourneyListCache;

pub struct AppState {
    pub db: PgPool,
    pub config: Config,
    pub r2: R2,
    pub journeys_cache: JourneyListCache,
}

pub type SharedState = Arc<AppState>;
