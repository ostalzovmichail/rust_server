pub mod handlers;
pub mod models;
pub mod router;

use crate::core::config::Config;
use crate::core::db::Database;
use std::sync::Arc;

/// Глобальное состояние приложения, разделяемое всеми обработчиками.
///
/// `Arc<dyn Database>` — чтобы подменить СУБД без правок в обработчиках.
/// `Arc<Config>` — конфиг нужен роутеру (Swagger/CORS), и он неизменяем.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<dyn Database>,
    pub config: Arc<Config>,
}

impl AppState {
    /// Оборачивает зависимости в `Arc`: создаётся один раз в `main`,
    /// дальше копируется между задачами дёшево (атомарный счётчик).
    pub fn new(db: impl Database + 'static, config: Config) -> Self {
        Self {
            db: Arc::new(db),
            config: Arc::new(config),
        }
    }
}
