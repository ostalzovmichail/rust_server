use std::env;

/// Поддерживаемые окружения приложения.
///
/// Окружение выбирается переменной `APP_ENV` ("dev" по умолчанию) и влияет
/// на логику, которую нельзя смешивать в разных местах кода: фильтр логирования,
/// доступность Swagger UI, поведение CORS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Development,
    Production,
}

impl Environment {
    /// Разбирает строку из `APP_ENV`, допуская и короткие, и полные имена.
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "dev" | "development" => Ok(Self::Development),
            "prod" | "production" => Ok(Self::Production),
            other => Err(format!(
                "Неизвестное значение APP_ENV='{other}'. Допустимые значения: dev, prod"
            )),
        }
    }

    /// Короткое имя окружения — используется и для имени файла `.env.{name}`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Development => "dev",
            Self::Production => "prod",
        }
    }

    pub fn is_dev(self) -> bool {
        matches!(self, Self::Development)
    }
}

/// Конфигурация приложения, загружаемая из переменных окружения.
///
/// Валидация выполняется один раз при старте: если конфигурация невалидна,
/// процесс завершается сразу (`Config::load` возвращает `Err` с понятным
/// сообщением), а не падает где-то посреди работы.
#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub server_host: String,
    pub server_port: u16,
    pub start_message: String,
    /// Разрешённые CORS-origin'ы. Используются только в prod;
    /// в dev включается permissive-режим (см. `api::router::cors_layer`).
    pub cors_allowed_origins: Vec<String>,
    pub environment: Environment,
}

impl Config {
    /// Загружает конфигурацию из окружения и файлов `.env.*`.
    pub fn load() -> Result<Self, String> {
        // Шаг 1: определяем окружение. Оно нужно ещё до загрузки .env-файлов,
        // потому что имя файла зависит от него.
        let environment = match env::var("APP_ENV") {
            Ok(raw) => Environment::parse(&raw)?,
            // APP_ENV не задан — используем значение по умолчанию.
            Err(_) => Environment::Development,
        };

        // Шаг 2: подключаем файлы окружения.
        Self::load_env_files(environment);

        // Шаг 3: читаем и валидируем переменные.
        Ok(Self {
            database_url: require_var("DATABASE_URL")?,
            server_host: require_var("SERVER_HOST")?,
            server_port: parse_port(&require_var("SERVER_PORT")?)?,
            start_message: require_var("START_MESSAGE")?,
            cors_allowed_origins: parse_origin_list(
                &env::var("CORS_ALLOWED_ORIGINS").unwrap_or_default(),
            ),
            environment,
        })
    }

    /// Включена ли интерактивная документация (Swagger UI).
    /// В проде документация недоступна: она раскрывает поверхность API.
    pub fn docs_enabled(&self) -> bool {
        self.environment.is_dev()
    }

    /// Компоненты адреса для привязки TCP-слушателя.
    pub fn bind_addr(&self) -> (&str, u16) {
        (&self.server_host, self.server_port)
    }

    /// Загружает `.env.{APP_ENV}`, затем `.env` как резервный файл.
    ///
    /// dotenvy не перезаписывает уже заданные переменные, поэтому приоритет
    /// такой: системное окружение > `.env.{APP_ENV}` > `.env`.
    fn load_env_files(environment: Environment) {
        let _ = dotenvy::from_filename(format!(".env.{}", environment.name()));
        let _ = dotenvy::dotenv();
    }

    /// Конфигурация для юнит-тестов: изолированная in-memory БД, dev-режим.
    #[cfg(test)]
    pub fn for_tests() -> Self {
        Self {
            database_url: "sqlite::memory:".to_string(),
            server_host: "127.0.0.1".to_string(),
            server_port: 0,
            start_message: "test".to_string(),
            cors_allowed_origins: Vec::new(),
            environment: Environment::Development,
        }
    }
}

/// Читает обязательную переменную окружения; её отсутствие — ошибка конфигурации.
fn require_var(name: &str) -> Result<String, String> {
    env::var(name).map_err(|_| {
        format!("Переменная {name} не задана: укажите её в окружении или в файле .env")
    })
}

/// `SERVER_PORT` хранится строкой, но нужен как число — валидируем сразу.
fn parse_port(raw: &str) -> Result<u16, String> {
    raw.parse().map_err(|_| {
        format!("SERVER_PORT='{raw}' не является портом (ожидается число от 0 до 65535)")
    })
}

/// Разбирает список origin'ов из `CORS_ALLOWED_ORIGINS="https://a.ru, https://b.ru"`.
/// Пустые элементы отбрасываются, чтобы двойные запятые не давали мусор.
fn parse_origin_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(str::to_string)
        .collect()
}
