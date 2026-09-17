//! Данные для индикаторов панели.
//!
//! Всё читается из `/sys` — без демонов и без опроса внешних программ. Каждая
//! функция принимает корневой каталог параметром, поэтому проверяется тестами
//! на поддельном дереве файлов.

use std::path::{Path, PathBuf};

/// Состояние батареи.
#[derive(Debug, Clone, PartialEq)]
pub struct Battery {
    /// Заряд в процентах.
    pub percent: u8,
    pub charging: bool,
}

impl Battery {
    /// Имя значка, соответствующее заряду.
    pub fn icon_name(&self) -> &'static str {
        if self.charging {
            return "battery-good-charging-symbolic";
        }
        match self.percent {
            0..=10 => "battery-empty-symbolic",
            11..=30 => "battery-caution-symbolic",
            31..=60 => "battery-low-symbolic",
            61..=90 => "battery-good-symbolic",
            _ => "battery-full-symbolic",
        }
    }

    /// Подпись рядом со значком.
    pub fn label(&self) -> String {
        format!("{}%", self.percent)
    }
}

/// Читает состояние батареи из дерева `/sys/class/power_supply`.
///
/// Возвращает `None`, если батареи нет: на настольной машине индикатор просто
/// не показывается.
pub fn read_battery(sys_root: &Path) -> Option<Battery> {
    let supply = sys_root.join("class/power_supply");
    let entries = std::fs::read_dir(&supply).ok()?;

    let mut batteries: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            // Блок питания тоже лежит здесь, но у него нет ёмкости.
            path.file_name()
                .map(|name| name.to_string_lossy().starts_with("BAT"))
                .unwrap_or(false)
        })
        .collect();
    // Порядок чтения каталога произволен: сортируем, чтобы при двух батареях
    // индикатор не прыгал между ними.
    batteries.sort();

    let path = batteries.first()?;
    let percent: u8 = read_trimmed(&path.join("capacity"))?.parse().ok()?;
    let status = read_trimmed(&path.join("status")).unwrap_or_default();

    Some(Battery {
        percent: percent.min(100),
        charging: status.eq_ignore_ascii_case("charging"),
    })
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
}

/// Громкость звука.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Volume {
    /// Уровень от 0 до 1 (может быть выше при усилении).
    pub level: f64,
    pub muted: bool,
}

impl Volume {
    /// Значок, соответствующий громкости.
    pub fn icon_name(&self) -> &'static str {
        if self.muted || self.level <= 0.001 {
            return "audio-volume-muted-symbolic";
        }
        match (self.level * 100.0) as u32 {
            0..=33 => "audio-volume-low-symbolic",
            34..=66 => "audio-volume-medium-symbolic",
            _ => "audio-volume-high-symbolic",
        }
    }

    pub fn percent(&self) -> u32 {
        (self.level * 100.0).round() as u32
    }
}

/// Разбирает вывод `wpctl get-volume @DEFAULT_AUDIO_SINK@`.
///
/// PipeWire печатает строку вида `Volume: 0.65` или `Volume: 0.65 [MUTED]`.
pub fn parse_volume(output: &str) -> Option<Volume> {
    let line = output.lines().find(|line| line.contains("Volume:"))?;
    let rest = line.split("Volume:").nth(1)?;
    let level: f64 = rest.split_whitespace().next()?.parse().ok()?;

    Some(Volume {
        level: level.max(0.0),
        muted: line.contains("[MUTED]"),
    })
}

/// Спрашивает у системы текущую громкость.
///
/// Возвращает `None`, если звуковой сервер не отвечает: на машине без звука
/// индикатор просто не показывается.
pub fn read_volume() -> Option<Volume> {
    let output = std::process::Command::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_volume(&String::from_utf8_lossy(&output.stdout))
}

/// Задаёт громкость.
pub fn set_volume(level: f64) {
    let level = level.clamp(0.0, 1.0);
    let _ = std::process::Command::new("wpctl")
        .args(["set-volume", "@DEFAULT_AUDIO_SINK@", &format!("{level:.2}")])
        .status();
}

/// Укорачивает заголовок окна до разумной длины.
///
/// Заголовки вроде «документ.txt — Правка — Редактор» растягивают панель и
/// выдавливают часы, поэтому длинный текст обрезается с многоточием.
pub fn shorten_title(title: &str, max_chars: usize) -> String {
    let chars: Vec<char> = title.chars().collect();
    if chars.len() <= max_chars {
        return title.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let keep = max_chars.saturating_sub(1);
    format!("{}…", chars[..keep].iter().collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeSys(PathBuf);

    impl FakeSys {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("hype-shell-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            Self(path)
        }

        fn battery(&self, name: &str, capacity: &str, status: &str) -> &Self {
            let dir = self.0.join("class/power_supply").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("capacity"), capacity).unwrap();
            std::fs::write(dir.join("status"), status).unwrap();
            self
        }

        fn adapter(&self, name: &str) -> &Self {
            let dir = self.0.join("class/power_supply").join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("online"), "1").unwrap();
            self
        }
    }

    impl Drop for FakeSys {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_a_discharging_battery() {
        let sys = FakeSys::new("discharge");
        sys.battery("BAT0", "47\n", "Discharging\n");

        let battery = read_battery(&sys.0).unwrap();
        assert_eq!(
            battery,
            Battery {
                percent: 47,
                charging: false
            }
        );
        assert_eq!(battery.label(), "47%");
    }

    #[test]
    fn recognises_charging() {
        let sys = FakeSys::new("charging");
        sys.battery("BAT0", "80", "Charging");
        let battery = read_battery(&sys.0).unwrap();
        assert!(battery.charging);
        assert_eq!(battery.icon_name(), "battery-good-charging-symbolic");
    }

    #[test]
    fn the_power_adapter_is_not_mistaken_for_a_battery() {
        let sys = FakeSys::new("adapter");
        sys.adapter("AC");
        assert_eq!(read_battery(&sys.0), None);
    }

    #[test]
    fn with_two_batteries_the_first_one_is_used_consistently() {
        let sys = FakeSys::new("two");
        sys.battery("BAT1", "20", "Discharging");
        sys.battery("BAT0", "90", "Discharging");
        assert_eq!(read_battery(&sys.0).unwrap().percent, 90);
    }

    #[test]
    fn a_machine_without_batteries_shows_nothing() {
        let sys = FakeSys::new("desktop");
        std::fs::create_dir_all(sys.0.join("class/power_supply")).unwrap();
        assert_eq!(read_battery(&sys.0), None);
    }

    #[test]
    fn a_missing_sys_tree_is_not_an_error() {
        assert_eq!(read_battery(Path::new("/нет/такого")), None);
    }

    #[test]
    fn nonsense_capacity_is_ignored_rather_than_shown() {
        let sys = FakeSys::new("garbage");
        sys.battery("BAT0", "неизвестно", "Discharging");
        assert_eq!(read_battery(&sys.0), None);
    }

    #[test]
    fn capacity_above_a_hundred_is_capped() {
        let sys = FakeSys::new("overflow");
        sys.battery("BAT0", "127", "Full");
        assert_eq!(read_battery(&sys.0).unwrap().percent, 100);
    }

    #[test]
    fn battery_icons_cover_the_whole_range() {
        let icon = |percent| {
            Battery {
                percent,
                charging: false,
            }
            .icon_name()
        };
        assert_eq!(icon(5), "battery-empty-symbolic");
        assert_eq!(icon(25), "battery-caution-symbolic");
        assert_eq!(icon(50), "battery-low-symbolic");
        assert_eq!(icon(75), "battery-good-symbolic");
        assert_eq!(icon(100), "battery-full-symbolic");
    }

    #[test]
    fn volume_is_parsed_from_pipewire_output() {
        let volume = parse_volume("Volume: 0.65\n").unwrap();
        assert!((volume.level - 0.65).abs() < 1e-9);
        assert!(!volume.muted);
        assert_eq!(volume.percent(), 65);
    }

    #[test]
    fn a_muted_sink_is_recognised() {
        let volume = parse_volume("Volume: 0.40 [MUTED]").unwrap();
        assert!(volume.muted);
        assert_eq!(volume.icon_name(), "audio-volume-muted-symbolic");
    }

    #[test]
    fn volume_icons_follow_the_level() {
        let icon = |level| {
            Volume {
                level,
                muted: false,
            }
            .icon_name()
        };
        assert_eq!(icon(0.0), "audio-volume-muted-symbolic");
        assert_eq!(icon(0.2), "audio-volume-low-symbolic");
        assert_eq!(icon(0.5), "audio-volume-medium-symbolic");
        assert_eq!(icon(0.9), "audio-volume-high-symbolic");
    }

    #[test]
    fn nonsense_volume_output_is_ignored() {
        assert!(parse_volume("").is_none());
        assert!(parse_volume("Node not found").is_none());
        assert!(parse_volume("Volume: громко").is_none());
    }

    #[test]
    fn long_titles_are_shortened_with_an_ellipsis() {
        assert_eq!(shorten_title("короткий", 20), "короткий");
        // Многоточие входит в лимит, поэтому букв остаётся на одну меньше.
        let short = shorten_title("очень длинный заголовок окна", 10);
        assert_eq!(short, "очень дли…");
        assert_eq!(short.chars().count(), 10);
        // Считаются символы, а не байты: кириллица не должна резаться посреди
        // буквы.
        assert_eq!(shorten_title("абвгд", 3).chars().count(), 3);
        assert_eq!(shorten_title("абвгд", 0), "");
    }
}
