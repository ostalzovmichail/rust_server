use crate::api::models::{internal_error, ok, ApiResult};
use crate::api::AppState;
use axum::extract::State;

/// GET / — базовая проверка, что сервер отвечает.
///
/// Эндпоинту не нужен доступ к состоянию — сигнатура без `State`,
/// что сразу видно по коду и не требует лишних деструктуризаций.
#[utoipa::path(
    get,
    path = "/",
    responses(
        (status = 200, description = "Успешный ответ", body = ApiResponse)
    ),
    tag = "Basic"
)]
pub async fn home_handler() -> ApiResult {
    ok("привет дом")
}

/// GET /test — тестовый эндпоинт.
#[utoipa::path(
    get,
    path = "/test",
    responses(
        (status = 200, description = "Успешный ответ", body = ApiResponse)
    ),
    tag = "Basic"
)]
pub async fn test_handler() -> ApiResult {
    ok("привет тест")
}

/// GET /db — проверка подключения к базе данных.
///
/// Версия SQLite наружу не возвращается: точная версия СУБД — это
/// «отпечаток», по которому подбирают известные уязвимости. Она пишется
/// в лог на стороне сервера, клиент видит только факт успеха/ошибки.
#[utoipa::path(
    get,
    path = "/db",
    responses(
        (status = 200, description = "БД доступна", body = ApiResponse),
        (status = 500, description = "Ошибка БД", body = ApiResponse)
    ),
    tag = "Basic"
)]
pub async fn db_handler(State(state): State<AppState>) -> ApiResult {
    match state.db.get_version().await {
        Ok(version) => {
            tracing::debug!(version = %version, "Проверка БД: подключение в порядке");
            ok("База данных подключена")
        }
        Err(error) => {
            // Детали ошибки — только в лог; в ответ клиенту идёт generic-сообщение.
            tracing::error!(error = %error, "Проверка БД: не удалось получить версию SQLite");
            internal_error()
        }
    }
}
