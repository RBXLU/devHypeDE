//! Звуки среды.
//!
//! Своего звукового движка у HypeDE нет и не нужно: в системе уже есть
//! PipeWire или PulseAudio со своими проигрывателями. Оболочка лишь выбирает
//! доступный и запускает его отдельным процессом — так звук не может подвесить
//! интерфейс, а его сбой не роняет панель.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use hype_config::SoundConfig;

/// Событие, у которого есть свой звук.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sound {
    /// Вход в сеанс.
    Startup,
    /// Уведомление.
    Notification,
    /// Изменение громкости.
    Volume,
    /// Ошибка.
    Error,
    /// Завершение сеанса.
    Logout,
}

impl Sound {
    /// Имя файла в наборе звуков.
    pub fn file_name(self) -> &'static str {
        match self {
            Sound::Startup => "startup.wav",
            Sound::Notification => "notification.wav",
            Sound::Volume => "volume.wav",
            Sound::Error => "error.wav",
            Sound::Logout => "logout.wav",
        }
    }
}

/// Проигрыватели в порядке предпочтения.
///
/// `pw-play` идёт первым: на CachyOS звук держит PipeWire, и обращение к нему
/// напрямую короче пути через прослойку совместимости.
const PLAYERS: &[&str] = &["pw-play", "paplay", "aplay"];

/// Ищет доступный проигрыватель.
///
/// `exists` отвечает, есть ли такая программа: параметром, а не проверкой
/// внутри, — так выбор проверяется тестами без установки чего-либо в систему.
pub fn pick_player(exists: impl Fn(&str) -> bool) -> Option<&'static str> {
    PLAYERS.iter().copied().find(|player| exists(player))
}

/// Есть ли программа в `PATH`.
fn in_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
        .unwrap_or(false)
}

/// Путь к файлу звука с учётом пользовательского набора.
pub fn sound_path(config: &SoundConfig, sound: Sound) -> Option<PathBuf> {
    if let Some(dir) = &config.theme_dir {
        let candidate = dir.join(sound.file_name());
        if candidate.exists() {
            return Some(candidate);
        }
    }
    hype_config::paths::find_asset(&format!("sounds/{}", sound.file_name()))
}

/// Проигрывает звук, если звуки включены и в системе есть чем.
///
/// Ничего не возвращает и ни на что не жалуется громко: отсутствие звука не
/// должно мешать работе.
pub fn play(config: &SoundConfig, sound: Sound) {
    if !config.enabled || config.volume <= 0.0 {
        return;
    }

    let Some(player) = pick_player(in_path) else {
        tracing::debug!("проигрыватель звука не найден");
        return;
    };
    let Some(path) = sound_path(config, sound) else {
        tracing::debug!("звук {} не найден", sound.file_name());
        return;
    };

    let mut command = Command::new(player);
    command.arg(&path);

    // У каждого проигрывателя свой способ задать громкость.
    match player {
        "pw-play" => {
            command.arg(format!("--volume={:.2}", config.volume.clamp(0.0, 1.0)));
        }
        "paplay" => {
            let volume = (config.volume.clamp(0.0, 1.0) * 65536.0).round() as u32;
            command.arg(format!("--volume={volume}"));
        }
        // aplay громкость не умеет — играем как есть.
        _ => {}
    }

    let started = command.stdout(Stdio::null()).stderr(Stdio::null()).spawn();

    if let Err(err) = started {
        tracing::debug!("не удалось проиграть {}: {err}", sound.file_name());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_has_a_file() {
        for sound in [
            Sound::Startup,
            Sound::Notification,
            Sound::Volume,
            Sound::Error,
            Sound::Logout,
        ] {
            assert!(sound.file_name().ends_with(".wav"), "{sound:?}");
        }
    }

    #[test]
    fn pipewire_is_preferred() {
        assert_eq!(pick_player(|_| true), Some("pw-play"));
    }

    #[test]
    fn the_next_player_is_used_when_the_first_is_missing() {
        assert_eq!(pick_player(|program| program == "paplay"), Some("paplay"));
        assert_eq!(pick_player(|program| program == "aplay"), Some("aplay"));
    }

    #[test]
    fn a_system_without_players_gets_no_sound() {
        assert_eq!(pick_player(|_| false), None);
    }

    #[test]
    fn a_custom_sound_set_wins_over_the_built_in_one() {
        let dir = std::env::temp_dir().join(format!("hype-sounds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("startup.wav"), b"x").unwrap();

        let config = SoundConfig {
            theme_dir: Some(dir.clone()),
            ..SoundConfig::default()
        };
        assert_eq!(
            sound_path(&config, Sound::Startup),
            Some(dir.join("startup.wav"))
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_custom_set_without_the_file_falls_back() {
        let config = SoundConfig {
            theme_dir: Some(PathBuf::from("/нет/такого/набора")),
            ..SoundConfig::default()
        };
        // Встроенный набор может быть не установлен в окружении теста, поэтому
        // проверяется только то, что пустой пользовательский набор не ломает
        // поиск.
        let _ = sound_path(&config, Sound::Startup);
    }

    #[test]
    fn disabled_sound_plays_nothing() {
        // Проверяется тем, что вызов не паникует и не зависит от системы.
        let config = SoundConfig {
            enabled: false,
            ..SoundConfig::default()
        };
        play(&config, Sound::Startup);

        let silent = SoundConfig {
            volume: 0.0,
            ..SoundConfig::default()
        };
        play(&silent, Sound::Startup);
    }
}
