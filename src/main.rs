mod api;
mod core;

use crate::api::AppState;
use crate::core::config::Config;
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    // 1. Конфигурация. Невалидный конфиг — выходим сразу с понятным сообщением,
    // а не падаем где-то посреди работы сервера.
    let config = Config::load().unwrap_or_else(|error| {
        eprintln!("Ошибка конфигурации: {error}");
        std::process::exit(1);
    });

    // 2. Логирование инициализируется после конфига: от него зависит фильтр уровней.
    // guard-объекты должны жить до конца main (иначе буфер логов не допишется).
    let _log_guards = core::logging::init_logging(&config);

    // 3. База данных: подключение + автоматические миграции.
    // Ошибка здесь — фатальна, сервер без БД работать не может.
    let db = core::db::SqliteDb::new(&config.database_url)
        .await
        .unwrap_or_else(|error| {
            // eprintln вместо tracing: std::process::exit не вызывает
            // деструкторы, и накопленный буфер non-blocking логгера потерялся бы.
            eprintln!("Не удалось подключиться к базе данных: {error}");
            std::process::exit(1);
        });

    let (host, port) = config.bind_addr();
    tracing::info!(
        env = config.environment.name(),
        host,
        port,
        message = %config.start_message,
        "Сервер запускается"
    );

    // 4. Привязка адреса. bind((host, port)) принимает и IP, и имя хоста
    // ("localhost") — форматирование строки здесь не нужно.
    let listener = TcpListener::bind((host, port))
        .await
        .unwrap_or_else(|error| {
            eprintln!("Не удалось привязаться к адресу {host}:{port}: {error}");
            std::process::exit(1);
        });

    // 5. Роутер собирается уже с готовым состоянием (БД + конфиг).
    // Здесь конфиг переезжает в AppState, поэтому привязка listener'а — выше.
    let app = api::router::create_router(AppState::new(db, config));

    // 6. Запуск. with_graceful_shutdown даёт время дообработать текущие
    // запросы по SIGINT/SIGTERM (Ctrl+C или остановка контейнера) —
    // без этого соединения обрываются, а ответы теряются.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Сервер неожиданно завершил работу");
}

/// Ожидает сигнал завершения: Ctrl+C (все ОС) или SIGTERM (Unix).
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Не удалось подписаться на сигнал Ctrl+C");
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{signal, SignalKind};
        signal(SignalKind::terminate())
            .expect("Не удалось подписаться на SIGTERM")
            .recv()
            .await;
    };

    // На не-Unix SIGTERM не существует — ждём только Ctrl+C.
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("Получен сигнал остановки — сервер завершает работу");
}
