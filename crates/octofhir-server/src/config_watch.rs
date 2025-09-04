use std::{path::PathBuf, sync::{Arc, RwLock, Mutex}, time::{Duration, Instant}};

use notify::{recommended_watcher, Event, RecursiveMode, Watcher};
use tracing::{error, info, warn};

use crate::config::{loader, AppConfig};

/// Start watching a configuration file for changes with a simple debounce (500ms).
/// On change, it will attempt to reload and validate configuration. If successful,
/// it updates the shared config and applies hot-reloadable settings (e.g., logging level).
///
/// Returns an optional thread join guard to keep the watcher alive.
pub fn start_config_watcher(path: PathBuf, shared: Arc<RwLock<AppConfig>>) -> Option<std::thread::JoinHandle<()>> {
    if !path.exists() {
        warn!("config file {:?} does not exist; watcher disabled", path);
        return None;
    }

    let handle = std::thread::spawn(move || {
        let last_reload = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
        let lp = path.clone();
        let lr = last_reload.clone();

        let mut watcher = match recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(_event) => {
                    let mut last = lr.lock().unwrap();
                    let now = Instant::now();
                    if now.duration_since(*last) >= Duration::from_millis(500) {
                        *last = now;
                        match loader::load_config(lp.to_str()) {
                            Ok(new_cfg) => {
                                // Apply hot-reloadable settings
                                crate::observability::apply_logging_level(&new_cfg.logging.level);
                                crate::observability::apply_otel_config(&new_cfg.otel);
                                // Replace shared config
                                if let Ok(mut guard) = shared.write() {
                                    *guard = new_cfg;
                                }
                                info!("configuration reloaded successfully");
                            }
                            Err(e) => {
                                error!("configuration reload failed: {}", e);
                            }
                        }
                    }
                }
                Err(e) => error!("watch error: {}", e),
            }
        }) {
            Ok(w) => w,
            Err(e) => {
                error!("failed to start config watcher: {}", e);
                return;
            }
        };

        if let Err(e) = watcher.watch(&path, RecursiveMode::NonRecursive) {
            error!("failed to watch config file: {}", e);
            return;
        }

        // Keep thread alive forever
        loop { std::thread::park(); }
    });

    Some(handle)
}
