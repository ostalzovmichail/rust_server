use crate::api::handlers::basic;
use crate::api::models::{ApiResponse, ResponseStatus};
use crate::api::AppState;
use crate::core::config::Config;
use axum::{
    http::{header, HeaderValue, Method},
    routing::get,
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

/// OpenAPI-спецификация всего сервера.
#[derive(OpenApi)]
#[openapi(
    paths(
        basic::home_handler,
        basic::test_handler,
        basic::db_handler
    ),
    components(
        // ResponseStatus перечисляем явно: без этого Swagger UI покажет
        // битую ссылку на схему поля `status`.
        schemas(ApiResponse, ResponseStatus)
    ),
    tags(
        (name = "Basic", description = "Базовые эндпоинты сервера")
    )
)]
struct ApiDoc;

/// Собирает роутер сервера: маршруты + (в dev) Swagger + слои CORS и трассировки.
pub fn create_router(state: AppState) -> Router {
    // Флаги читаем до перемещения состояния в роутер (`with_state` забирает его).
    let docs_enabled = state.config.docs_enabled();
    let cors = cors_layer(&state.config);

    let mut router = Router::new().merge(api_routes());
    if docs_enabled {
        router = router.merge(docs_route());
    }

    router
        .with_state(state)
        // Слои применяются ко всем маршрутам, включая Swagger.
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}

/// Маршруты API. Вынесены в отдельную функцию, чтобы `create_router`
/// читалась как последовательность настройки, а не как одна длинная цепочка.
fn api_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(basic::home_handler))
        .route("/test", get(basic::test_handler))
        .route("/db", get(basic::db_handler))
}

/// Swagger UI. Подключается только в dev (см. `Config::docs_enabled`):
/// в проде документация раскрывает поверхность API.
fn docs_route() -> SwaggerUi {
    SwaggerUi::new("/swagger-ui").url("/swagger-ui/openapi.json", ApiDoc::openapi())
}

/// Настройка CORS в зависимости от окружения.
///
/// * dev — `permissive`: любой origin и любой метод, чтобы фронт/Swagger
///   на другом порту работали без возни.
/// * prod со списком origin'ов — отвечаем только доверенным.
/// * prod без списка — CORS-заголовки не отдаём вообще: браузеры сами
///   блокируют чужие origin'ы (безопасно по умолчанию).
fn cors_layer(config: &Config) -> CorsLayer {
    if config.environment.is_dev() {
        return CorsLayer::permissive();
    }

    if config.cors_allowed_origins.is_empty() {
        return CorsLayer::new();
    }

    let origins: Vec<HeaderValue> = config
        .cors_allowed_origins
        .iter()
        .filter_map(|raw| match raw.parse() {
            Ok(origin) => Some(origin),
            Err(_) => {
                // Некорректный origin пропускаем, но обязательно логируем:
                // молча терять настройки администратора нельзя.
                tracing::warn!(origin = %raw, "Пропущен некорректный origin в CORS_ALLOWED_ORIGINS");
                None
            }
        })
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET])
        .allow_headers([header::CONTENT_TYPE])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::db::{Database, SqliteDb};
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt; // ServiceExt::oneshot — вызов роутера без TCP

    /// Собирает тестовое приложение поверх in-memory SQLite:
    /// тесты не трогают реальные файлы БД.
    async fn test_app() -> Router {
        let db = SqliteDb::new("sqlite::memory:")
            .await
            .expect("Не удалось создать in-memory БД");
        create_router(AppState::new(db, Config::for_tests()))
    }

    #[tokio::test]
    async fn test_home_handler() {
        let response = test_app()
            .await
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_test_handler() {
        let response = test_app()
            .await
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_db_handler() {
        let response = test_app()
            .await
            .oneshot(Request::builder().uri("/db").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    /// БД, которая всегда завершается ошибкой — нужно для проверки
    /// обработки сбоев (в проде так бывает, и сервер не должен падать).
    struct FailingDb;

    #[async_trait::async_trait]
    impl Database for FailingDb {
        async fn get_version(&self) -> Result<String, sqlx::Error> {
            Err(sqlx::Error::PoolTimedOut)
        }
    }

    #[tokio::test]
    async fn test_db_handler_returns_500_and_hides_details() {
        let app = create_router(AppState::new(FailingDb, Config::for_tests()));

        let response = app
            .oneshot(Request::builder().uri("/db").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        // Внутренности ошибки (пути, SQL) не должны утекать клиенту.
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["message"], "Внутренняя ошибка сервера");
    }
}
