use std::path::PathBuf;

use log::LevelFilter;

#[derive(Debug, Clone)]
pub struct LoggerOptions {
    pub log_level: LevelFilter,
    pub log_file: Option<PathBuf>,
}

impl Default for LoggerOptions {
    fn default() -> Self {
        LoggerOptions {
            log_level: LevelFilter::Info,
            log_file: Some(PathBuf::from("./output.log")),
        }
    }
}

pub fn attach_default_system_logger() -> anyhow::Result<()> {
    attach_system_logger(LoggerOptions::default())
}

pub fn attach_system_logger(options: LoggerOptions) -> anyhow::Result<()> {
    let mut dispatcher = fern::Dispatch::new()
        .format(move |out, message, record| {
            out.finish(format_args!(
                "{{{}}} [{}/{}] {}",
                chrono::Local::now().format("%d/%m/%y %H:%M:%S"),
                record.target(),
                record.level(),
                message
            ))
        })
        .level(options.log_level)
        .chain(std::io::stdout());

    if let Some(path) = options.log_file.as_ref() {
        if path.exists() {
            std::fs::remove_file(path)?;
        }

        dispatcher = dispatcher.chain(fern::log_file(path)?);
    }

    dispatcher.apply()?;

    Ok(())
}
