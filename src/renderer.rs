use adw::prelude::*;
use std::ffi::{OsStr, OsString};
use std::sync::OnceLock;

const BOOTSTRAPPED: &str = "SCHOOL_DAY_RENDERER_BOOTSTRAPPED";
const SOURCE: &str = "SCHOOL_DAY_RENDERER_SOURCE";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Renderer {
    #[serde(rename = "opengl", alias = "gl")]
    OpenGl,
    Cairo,
    Vulkan,
    #[default]
    #[serde(other)]
    Auto,
}

impl Renderer {
    pub const LABELS: [&str; 4] = [
        "Автоматически - выбор GTK",
        "OpenGL - аппаратное ускорение",
        "Cairo - экономия памяти",
        "Vulkan - аппаратное ускорение",
    ];

    pub fn index(self) -> u32 {
        match self {
            Self::Auto => 0,
            Self::OpenGl => 1,
            Self::Cairo => 2,
            Self::Vulkan => 3,
        }
    }

    pub fn from_index(index: u32) -> Self {
        match index {
            1 => Self::OpenGl,
            2 => Self::Cairo,
            3 => Self::Vulkan,
            _ => Self::Auto,
        }
    }

    fn gtk_name(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::OpenGl => Some("opengl"),
            Self::Cairo => Some("cairo"),
            Self::Vulkan => Some("vulkan"),
        }
    }
}

struct Startup {
    requested: Option<OsString>,
    external: bool,
    failed: bool,
    safe: bool,
}

static STARTUP: OnceLock<Startup> = OnceLock::new();
static ACTUAL: OnceLock<String> = OnceLock::new();

fn request(saved: Renderer, external: Option<OsString>, safe: bool) -> Option<OsString> {
    if safe {
        Some("cairo".into())
    } else {
        external
            .filter(|name| !name.is_empty())
            .or_else(|| saved.gtk_name().map(OsString::from))
    }
}

#[cfg(unix)]
fn is_cairo(name: Option<&OsStr>) -> bool {
    name.and_then(OsStr::to_str)
        .is_some_and(|name| name.eq_ignore_ascii_case("cairo"))
}

/// Runs before GTK initialization, application registration, data locks and Windows mutexes.
pub fn prepare() -> Vec<OsString> {
    let args: Vec<_> = std::env::args_os().collect();
    let safe = args.iter().skip(1).any(|arg| arg == "--safe-renderer");
    let bootstrapped = std::env::var_os(BOOTSTRAPPED).as_deref() == Some(OsStr::new("1"));
    let inherited = std::env::var_os("GSK_RENDERER");
    let external = if bootstrapped {
        std::env::var_os(SOURCE).as_deref() == Some(OsStr::new("external"))
    } else {
        !safe && inherited.as_ref().is_some_and(|value| !value.is_empty())
    };
    // Reading here must not migrate or save settings before the data lock is acquired.
    let saved = crate::settings::load_startup_settings().unwrap_or_else(|error| {
        eprintln!("Рендерер: {error}. Используется выбор GTK.");
        crate::settings::Settings::default()
    });
    let requested = if bootstrapped {
        inherited.clone()
    } else {
        request(saved.renderer, inherited.clone(), safe)
    };
    #[cfg(unix)]
    let mut failed = false;
    #[cfg(not(unix))]
    let failed = false;

    #[cfg(unix)]
    if !bootstrapped
        && (requested != inherited
            || (is_cairo(requested.as_deref())
                && std::env::var_os("__GLX_VENDOR_LIBRARY_NAME").is_some()))
    {
        use std::os::unix::process::CommandExt;
        let error = match std::env::current_exe() {
            Ok(executable) => {
                let mut child = std::process::Command::new(executable);
                child.args(args.iter().skip(1));
                child.env(BOOTSTRAPPED, "1");
                child.env(SOURCE, if external { "external" } else { "settings" });
                if let Some(name) = &requested {
                    child.env("GSK_RENDERER", name);
                } else {
                    child.env_remove("GSK_RENDERER");
                }
                if is_cairo(requested.as_deref()) {
                    // libGLX can load NVIDIA in its ELF constructor, before main().
                    // exec replaces this process before any UI, locks or user edits exist.
                    child.env_remove("__GLX_VENDOR_LIBRARY_NAME");
                }
                child.exec()
            }
            Err(error) => error,
        };
        // Do not retry indefinitely or prevent the original GTK fallback from opening.
        eprintln!("Не удалось применить рендерер при запуске: {error}");
        failed = true;
    }

    #[cfg(windows)]
    {
        // Windows environment mutation is thread-safe (std::env::set_var safety docs).
        // This still happens before GTK/GSK initialization. No mutex or updater changes.
        unsafe {
            if let Some(name) = &requested {
                std::env::set_var("GSK_RENDERER", name);
            } else {
                std::env::remove_var("GSK_RENDERER");
            }
        }
    }

    let _ = STARTUP.set(Startup {
        requested,
        external,
        failed,
        safe,
    });
    args.into_iter()
        .filter(|arg| arg != "--safe-renderer")
        .collect()
}

pub fn record_actual(window: &adw::ApplicationWindow) {
    if let Some(renderer) = window.renderer() {
        let name = renderer.type_().name();
        eprintln!("Графический рендерер: {name}");
        let _ = ACTUAL.set(name.to_string());
    }
}

fn actual_label(name: &str) -> &str {
    match name {
        "GskGLRenderer" | "GskNglRenderer" => "OpenGL",
        "GskVulkanRenderer" => "Vulkan",
        "GskCairoRenderer" => "Cairo",
        _ => "выбор GTK",
    }
}

pub fn status_text() -> String {
    let Some(actual) = ACTUAL.get() else {
        return String::new();
    };
    let mut text = String::new();
    if let Some(startup) = STARTUP.get() {
        let requested = startup.requested.as_deref().and_then(OsStr::to_str);
        let expected = match requested {
            Some("gl" | "opengl" | "ngl") => Some("OpenGL"),
            Some("vulkan") => Some("Vulkan"),
            Some("cairo") => Some("Cairo"),
            _ => None,
        };
        if startup.failed {
            text.push_str("Выбор не удалось применить при запуске. ");
        } else if expected.is_some_and(|expected| expected != actual_label(actual)) {
            text.push_str(&format!(
                "Запрошенный режим недоступен. Сейчас: {}. ",
                actual_label(actual)
            ));
        }
        if startup.external {
            text.push_str("Параметр GSK_RENDERER из окружения имеет приоритет над настройкой. ");
        }
        if startup.safe {
            text.push_str("Безопасный запуск; сохранённый выбор не изменён. ");
        }
    }
    text.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_mode_overrides_external_renderer_and_settings() {
        assert_eq!(
            request(Renderer::Vulkan, Some("opengl".into()), true),
            Some("cairo".into())
        );
    }

    #[test]
    fn external_override_is_preserved_and_empty_values_use_settings() {
        assert_eq!(
            request(Renderer::Cairo, Some("broadway".into()), false),
            Some("broadway".into())
        );
        assert_eq!(
            request(Renderer::OpenGl, Some("".into()), false),
            Some("opengl".into())
        );
        assert_eq!(request(Renderer::Auto, None, false), None);
    }

    #[test]
    fn unknown_preferences_recover_to_auto_and_all_modes_roundtrip() {
        assert_eq!(
            serde_json::from_str::<Renderer>("\"future-gpu\"").unwrap(),
            Renderer::Auto
        );
        for mode in [
            Renderer::Auto,
            Renderer::OpenGl,
            Renderer::Cairo,
            Renderer::Vulkan,
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(serde_json::from_str::<Renderer>(&json).unwrap(), mode);
        }
    }
}
