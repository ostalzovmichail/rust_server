use crate::core::config::{Config, Environment};
use chrono::Local;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use tracing::{Level, Metadata, Subscriber};
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{
    filter, fmt,
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};

/// Имена файлов логов. Хранятся в папке `logs/YYYY/MM/DD/` (ротация по дате).
const INFO_LOG_FILE: &str = "info.json";
const DEBUG_LOG_FILE: &str = "debug.json";
const OTHER_LOG_FILE: &str = "others.json";

/// Фильтры по умолчанию, если переменная `RUST_LOG` не задана.
/// В dev пишем подробно (включая SQL-запросы), в prod — только важное,
/// чтобы не засорять диск и не светить деталями запросов.
const DEV_DEFAULT_FILTER: &str = "debug";
const PROD_DEFAULT_FILTER: &str = "info,tower_http=warn";

/// Права доступа к файлам логов: только владелец (Unix).
/// Логи содержат URI запросов и тексты ошибок БД — посторонним они не нужны.
#[cfg(unix)]
const LOG_FILE_MODE: u32 = 0o600;

/// Инициализирует глобальный логгинг. Возвращает guard-объекты, которые
/// обязаны жить до конца `main`: их деструктор дожидается записи
/// накопленного буфера в файлы, без них последние логи теряются.
pub fn init_logging(config: &Config) -> Vec<WorkerGuard> {
    // Фильтр уровней: RUST_LOG имеет приоритет, иначе — дефолт для окружения.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_filter(config.environment)));

    // Создаём non-blocking writer'ы для трёх файлов (см. rolling_file_writer).
    let (info_writer, info_guard) = rolling_file_writer(INFO_LOG_FILE);
    let (debug_writer, debug_guard) = rolling_file_writer(DEBUG_LOG_FILE);
    let (other_writer, other_guard) = rolling_file_writer(OTHER_LOG_FILE);

    // Консоль: в dev выводим всё, в prod — только предупреждения и ошибки.
    let is_dev = config.environment.is_dev();
    let console_layer = fmt::layer()
        .with_writer(std::io::stdout)
        .with_filter(filter::filter_fn(move |meta| {
            is_dev || *meta.level() <= Level::WARN
        }));

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(json_file_layer(info_writer, is_info))
        .with(json_file_layer(debug_writer, is_debug))
        // "Прочее" — это WARN и ERROR (уровни `<= WARN` в tracing — именно они).
        .with(json_file_layer(other_writer, is_warning_or_worse))
        .init();

    vec![info_guard, debug_guard, other_guard]
}

/// Дефолтный фильтр логирования для окружения.
fn default_filter(environment: Environment) -> &'static str {
    if environment.is_dev() {
        DEV_DEFAULT_FILTER
    } else {
        PROD_DEFAULT_FILTER
    }
}

/// Оборачивает файловый writer в non-blocking режим: запись лога не блокирует
/// асинхронную задачу сервера — буфер сбрасывает отдельный поток.
fn rolling_file_writer(filename: &str) -> (NonBlocking, WorkerGuard) {
    tracing_appender::non_blocking(DirRollingWriter::new(filename))
}

/// JSON-слой, пишущий события, прошедшие предикат `predicate`, в отдельный файл.
///
/// Предикат передаётся как обычная функция (fn-указатель), поэтому все три
/// слоя имеют один и тот же тип и создаются одним helper'ом — без дублирования
/// и без потери производительности (вызов обычной функции, не замыкания).
fn json_file_layer<S>(
    writer: NonBlocking,
    predicate: fn(&Metadata<'_>) -> bool,
) -> impl Layer<S> + Send + Sync + 'static
where
    S: Subscriber + for<'span> LookupSpan<'span>,
{
    fmt::layer()
        .json()
        .with_writer(writer)
        .with_filter(filter::filter_fn(predicate))
}

/// Точечные предикаты уровней: каждый файл получает ровно свой уровень,
/// как и обещено в README (info.json — INFO, debug.json — DEBUG,
/// others.json — WARN и ERROR). Важно сравнивать уровень именно на равенство:
/// директива EnvFilter вроде "debug" пропускала бы сюда и INFO, и WARN.
fn is_info(meta: &Metadata<'_>) -> bool {
    *meta.level() == Level::INFO
}

fn is_debug(meta: &Metadata<'_>) -> bool {
    *meta.level() == Level::DEBUG
}

fn is_warning_or_worse(meta: &Metadata<'_>) -> bool {
    *meta.level() <= Level::WARN
}

/// Writer, который при каждой записи проверяет дату и при смене суток
/// открывает новый файл в папке `logs/YYYY/MM/DD/`.
struct DirRollingWriter {
    filename: String,
    /// Дата открытого файла (в формате YYYY-MM-DD); None — файл ещё не открывался.
    current_date: Option<String>,
    file: Option<File>,
}

impl DirRollingWriter {
    fn new(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
            current_date: None,
            file: None,
        }
    }

    /// Сегодняшняя дата в формате YYYY-MM-DD — по ней ротируем файл.
    fn today() -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    /// Возвращает открытый файл лога, при необходимости открывая новый
    /// (при наступлении новых суток).
    fn current_file(&mut self) -> io::Result<&mut File> {
        let today = Self::today();
        if self.current_date.as_deref() != Some(today.as_str()) {
            self.open_for_today(&today)?;
        }
        // Ответственность за открытие файла несёт open_for_today —
        // здесь None означает только внутреннюю ошибку.
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("файл лога не был открыт"))
    }

    /// Открывает (или переоткрывает) файл лога для текущих суток.
    fn open_for_today(&mut self, today: &str) -> io::Result<()> {
        let now = Local::now();
        // Собираем путь из частей: не требует ручного разделителя и
        // корректно работает на любой ОС.
        let path = PathBuf::from("logs")
            .join(now.format("%Y").to_string())
            .join(now.format("%m").to_string())
            .join(now.format("%d").to_string())
            .join(&self.filename);

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        restrict_permissions(&file);

        self.file = Some(file);
        self.current_date = Some(today.to_string());
        Ok(())
    }
}

impl Write for DirRollingWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.current_file()?.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        match &mut self.file {
            Some(file) => file.flush(),
            // Файл ещё не открывался — писать нечего, и открывать его
            // ради пустого flush'а не нужно.
            None => Ok(()),
        }
    }
}

/// Ограничивает права доступа к файлу логов (Unix).
#[cfg(unix)]
fn restrict_permissions(file: &File) {
    use std::os::unix::fs::PermissionsExt;
    // Ошибка здесь не должна ронять сервер: логирование — второстепенная задача.
    let _ = file.set_permissions(fs::Permissions::from_mode(LOG_FILE_MODE));
}

#[cfg(not(unix))]
fn restrict_permissions(_file: &File) {}
