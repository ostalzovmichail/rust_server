use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::time::Duration;

/// Максимальное количество соединений в пуле.
const MAX_CONNECTIONS: u32 = 5;
/// Сколько ждать свободное соединение, прежде чем вернуть ошибку.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(3);

/// Трейт операций с базой данных.
///
/// Обработчики depend от трейта, а не от конкретной реализации: чтобы
/// перейти с SQLite на PostgreSQL/MySQL, достаточно написать новую
/// реализацию трейта и не трогать остальной код (принцип Dependency Inversion).
#[async_trait::async_trait]
pub trait Database: Send + Sync {
    /// Возвращает версию SQLite — полезно как быстрая проверка живости БД.
    async fn get_version(&self) -> Result<String, sqlx::Error>;
    // Сюда будем добавлять методы типа create_user, get_user и т.д.
}

/// Реализация [`Database`] поверх пула соединений SQLite.
pub struct SqliteDb {
    pool: SqlitePool,
}

impl SqliteDb {
    /// Подключается к БД по `database_url` и прогоняет миграции.
    pub async fn new(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePoolOptions::new()
            .max_connections(MAX_CONNECTIONS)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect(database_url)
            .await?;

        // Миграции встроены в бинарь макросом `migrate!` (папка ./migrations)
        // и выполняются при старте. Это исключает подмену SQL-файлов на диске.
        sqlx::migrate!("./migrations").run(&pool).await?;

        Ok(Self { pool })
    }
}

#[async_trait::async_trait]
impl Database for SqliteDb {
    async fn get_version(&self) -> Result<String, sqlx::Error> {
        // Запрос статический и без подстановок — SQL-инъекция исключена.
        // Если появятся параметры, используем `sqlx::query_as(...).bind(...)`,
        // а не форматирование строки.
        let row: (String,) = sqlx::query_as("SELECT sqlite_version()")
            .fetch_one(&self.pool)
            .await?;
        Ok(row.0)
    }
}
