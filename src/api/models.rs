use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;

/// Тип ответа любого эндпоинта: код HTTP + JSON-тело.
/// Алиас избавляет обработчики от повторяющегося кортежа `(StatusCode, Json<...>)`.
pub type ApiResult = (StatusCode, Json<ApiResponse>);

/// Стандартное тело ответа сервера.
#[derive(Serialize, ToSchema)]
pub struct ApiResponse {
    /// Машиночитаемый статус: "success" или "error"
    pub status: ResponseStatus,
    /// Человекочитаемое сообщение
    pub message: String,
}

/// Статус в теле ответа.
///
/// Enum вместо строки: компилятор не даст вернуть произвольный текст,
/// а serde сериализует варианты ровно как раньше — "success"/"error".
#[derive(Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ResponseStatus {
    Success,
    Error,
}

impl ApiResponse {
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            status: ResponseStatus::Success,
            message: message.into(),
        }
    }

    pub fn error(message: impl Into<String>) -> Self {
        Self {
            status: ResponseStatus::Error,
            message: message.into(),
        }
    }
}

/// Успешный ответ: HTTP 200 + тело со статусом `success`.
pub fn ok(message: impl Into<String>) -> ApiResult {
    (StatusCode::OK, Json(ApiResponse::success(message)))
}

/// Ответ на внутреннюю ошибку сервера.
///
/// Клиенту отдаём только общее сообщение. Тексты ошибок БД, пути и SQL
/// уходят в лог (см. обработчики) — раскрывать их наружу нельзя:
/// это помогает злоумышленнику и учитывает детали инфраструктуры.
pub fn internal_error() -> ApiResult {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse::error("Внутренняя ошибка сервера")),
    )
}
