//! Слежение за файлами открытых вкладок. `notify` наблюдает КАТАЛОГ файла
//! (наблюдение за самим файлом не переживает атомарную замену «запись во
//! временный + rename», которой сохраняют редакторы); события фильтруются по
//! точному пути и дебаунсятся 300 мс — редакторы пишут сериями. Пропавшие
//! файлы раз в секунду перепроверяются (страховка от пропущенных событий).
//! Реакцию выдаёт `pump(now)`: время передаётся снаружи — дебаунс
//! тестируется без реального ожидания.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use notify::event::{ModifyKind, RenameMode};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

/// Тишина после последнего события, после которой реагируем.
pub const DEBOUNCE: Duration = Duration::from_millis(300);
/// Период перепроверки существования пропавших файлов.
pub const RECHECK: Duration = Duration::from_secs(1);

/// Реакция на изменение файла вкладки.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileEvent {
    /// Содержимое изменилось (или файл появился) — перечитать.
    Changed(PathBuf),
    /// Файл исчез — баннер «файл недоступен».
    Gone(PathBuf),
}

/// Сообщение из нити notify (упрощённое до пути + «изменился/исчез»).
enum PathMsg {
    Changed(PathBuf),
    Gone(PathBuf),
}

/// Дебаунс-состояние одного наблюдаемого файла.
struct Track {
    /// Дедлайн реакции; продлевается каждым новым событием.
    deadline: Option<Instant>,
    /// Файл отсутствует (последняя реакция Gone) — перепроверять.
    gone: bool,
    /// Следующая перепроверка пропавшего.
    probe: Instant,
}

pub struct Watcher {
    watcher: Option<RecommendedWatcher>,
    rx: Receiver<PathMsg>,
    /// Каталоги под наблюдением (нормализованные пути).
    dirs: Vec<PathBuf>,
    /// Наблюдаемые файлы: нормализованный путь → состояние.
    files: HashMap<PathBuf, Track>,
}

/// Нормализация пути для сравнения (Windows: без учёта регистра).
#[cfg(windows)]
fn norm(p: &Path) -> PathBuf {
    PathBuf::from(p.to_string_lossy().to_lowercase())
}

#[cfg(not(windows))]
fn norm(p: &Path) -> PathBuf {
    p.to_path_buf()
}

impl Watcher {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<PathMsg>();
        let watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(ev) = res {
                for msg in classify(&ev) {
                    let _ = tx.send(msg);
                }
            }
        })
        .map_err(|e| {
            log::warn!("файловый watcher не запустился: {e}");
            e
        })
        .ok();
        Self {
            watcher,
            rx,
            dirs: Vec::new(),
            files: HashMap::new(),
        }
    }

    /// Задаёт полный набор наблюдаемых файлов (пути вкладок). Каталоги
    /// наблюдения пересчитываются (родители файлов); новые файлы начинают
    /// наблюдаться, исчезнувшие из набора — забываются.
    pub fn set_files(&mut self, files: &[PathBuf]) {
        let want: Vec<PathBuf> = files.iter().map(|f| norm(f)).collect();
        let keys: Vec<PathBuf> = self.files.keys().cloned().collect();
        for k in keys {
            if !want.contains(&k) {
                self.files.remove(&k);
            }
        }
        for k in want {
            self.files.entry(k).or_insert(Track {
                deadline: None,
                gone: false,
                probe: Instant::now(),
            });
        }

        let mut dirs: Vec<PathBuf> = files
            .iter()
            .filter_map(|f| f.parent().map(|d| norm(d)))
            .collect();
        dirs.sort();
        dirs.dedup();
        if dirs != self.dirs {
            if let Some(w) = &mut self.watcher {
                for d in &self.dirs {
                    if !dirs.contains(d) {
                        let _ = w.unwatch(d);
                    }
                }
                for d in &dirs {
                    if !self.dirs.contains(d) {
                        if let Err(e) = w.watch(d, RecursiveMode::NonRecursive) {
                            log::warn!("наблюдение за {d:?} не удалось: {e}");
                        }
                    }
                }
            }
            self.dirs = dirs;
        }
    }

    /// Прокачка за кадр: принимает события, обновляет дебаунсы, выдаёт
    /// готовые реакции. Возвращает также время до ближайшего дедлайна —
    /// его надо отдать в `request_repaint_after`, иначе приложение уснёт и
    /// дедлайн не наступит. `now` передаётся снаружи (тестируемость).
    pub fn pump(&mut self, now: Instant) -> (Vec<FileEvent>, Option<Duration>) {
        loop {
            match self.rx.try_recv() {
                Ok(PathMsg::Changed(p)) => {
                    if let Some(t) = self.files.get_mut(&norm(&p)) {
                        t.deadline = Some(now + DEBOUNCE);
                    }
                }
                Ok(PathMsg::Gone(p)) => {
                    if let Some(t) = self.files.get_mut(&norm(&p)) {
                        t.deadline = Some(now + DEBOUNCE);
                    }
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }

        let mut out = Vec::new();
        let mut nearest: Option<Instant> = None;
        let keys: Vec<PathBuf> = self.files.keys().cloned().collect();
        for k in keys {
            let Some(t) = self.files.get_mut(&k) else { continue };
            if let Some(d) = t.deadline {
                if now >= d {
                    // Дебаунс истёк: истина о существовании — по файлу.
                    t.deadline = None;
                    let exists = std::fs::metadata(&k)
                        .map(|m| m.is_file())
                        .unwrap_or(false);
                    t.gone = !exists;
                    t.probe = now + RECHECK;
                    out.push(if exists {
                        FileEvent::Changed(k.clone())
                    } else {
                        FileEvent::Gone(k.clone())
                    });
                } else {
                    nearest = Some(nearest.map_or(d, |n: Instant| n.min(d)));
                }
            } else if t.gone && now >= t.probe {
                // Пропавший файл: перепроверка (страховка от пропущенных
                // событий каталога). Оживление гасит отложенный дебаунс —
                // перечитаем один раз, а не дважды.
                let exists =
                    std::fs::metadata(&k).map(|m| m.is_file()).unwrap_or(false);
                if exists {
                    t.gone = false;
                    t.deadline = None;
                    out.push(FileEvent::Changed(k.clone()));
                } else {
                    t.probe = now + RECHECK;
                    nearest = Some(nearest.map_or(t.probe, |n: Instant| n.min(t.probe)));
                }
            } else if t.gone {
                nearest = Some(nearest.map_or(t.probe, |n: Instant| n.min(t.probe)));
            }
        }
        (out, nearest.map(|n| n.saturating_duration_since(now)))
    }
}

/// Событие notify → упрощённые сообщения по путям. Переименование «both»
/// несёт два пути: старый исчез, новый появился.
fn classify(ev: &Event) -> Vec<PathMsg> {
    let ps = &ev.paths;
    match &ev.kind {
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) if ps.len() >= 2 => vec![
            PathMsg::Gone(ps[0].clone()),
            PathMsg::Changed(ps[1].clone()),
        ],
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
            ps.iter().map(|p| PathMsg::Gone(p.clone())).collect()
        }
        EventKind::Modify(ModifyKind::Name(RenameMode::To | RenameMode::Other)) => {
            ps.iter().map(|p| PathMsg::Changed(p.clone())).collect()
        }
        EventKind::Modify(_) | EventKind::Create(_) => {
            ps.iter().map(|p| PathMsg::Changed(p.clone())).collect()
        }
        EventKind::Remove(_) => ps.iter().map(|p| PathMsg::Gone(p.clone())).collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_path(name: &str) -> PathBuf {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("temp")
            .join(name);
        let _ = fs::remove_file(&p);
        p
    }

    /// Ждёт доставки события в канал (notify доставляет асинхронно). Кадки
    /// pump идут с фиксированным синтетическим временем `now` — событие
    /// принимается, но реакции раньше дедлайна не бывает.
    fn wait_ingested(w: &mut Watcher, now: Instant, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let (evs, until) = w.pump(now);
            assert!(evs.is_empty(), "при now до дедлайна реакций быть не может");
            if until.is_some() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    /// Ждёт реакции (реальным временем — дублей событий у notify хватает,
    /// каждый продлевает дедлайн). Первая реакция обязана быть `expect`.
    fn wait_reaction(w: &mut Watcher, expect: &FileEvent, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let (evs, _) = w.pump(Instant::now());
            if !evs.is_empty() {
                assert_eq!(evs, vec![expect.clone()]);
                return true;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        false
    }

    #[test]
    fn change_gone_reappear_lifecycle() {
        let path = temp_path("test-watcher.md");
        fs::write(&path, "# один").unwrap();
        let mut w = Watcher::new();
        w.set_files(&[path.clone()]);

        // Изменение: реакция не раньше дедлайна дебаунса (300 мс тишины).
        let t0 = Instant::now();
        fs::write(&path, "# два").unwrap();
        assert!(wait_ingested(&mut w, t0, Duration::from_secs(5)), "событие изменения не пришло");
        let (evs, _) = w.pump(t0 + DEBOUNCE - Duration::from_millis(50));
        assert!(evs.is_empty(), "реагировать раньше дедлайна нельзя");
        let expect = FileEvent::Changed(norm(&path));
        assert!(
            wait_reaction(&mut w, &expect, Duration::from_secs(3)),
            "реакция на изменение не пришла"
        );

        // Удаление → Gone (после дебаунса, по фактическому отсутствию файла).
        fs::remove_file(&path).unwrap();
        let t1 = Instant::now();
        assert!(wait_ingested(&mut w, t1, Duration::from_secs(5)), "событие удаления не пришло");
        let expect = FileEvent::Gone(norm(&path));
        assert!(
            wait_reaction(&mut w, &expect, Duration::from_secs(3)),
            "реакция на удаление не пришла"
        );

        // Появление снова — перепроверкой пропавших (RECHECK) или событием
        // Create из каталога: здесь качаем РЕАЛЬНЫМ временем — синтетически
        // замороженное держало бы дедлайн вечно будущим.
        fs::write(&path, "# четыре").unwrap();
        let mut reappeared = false;
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            let (evs, _) = w.pump(Instant::now());
            if evs.iter().any(|e| *e == FileEvent::Changed(norm(&path))) {
                reappeared = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(reappeared, "появление файла не замечено");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn set_files_drops_unwatched() {
        let a = temp_path("test-watcher-a.md");
        let b = temp_path("test-watcher-b.md");
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let mut w = Watcher::new();
        w.set_files(&[a.clone(), b.clone()]);
        w.set_files(&[a.clone()]);

        // События по b больше не отслеживаются: качаем секунду «кадрами» с
        // временем далеко за дебаунсом — реакций быть не должно.
        fs::remove_file(&b).unwrap();
        let far = Instant::now() + Duration::from_secs(10);
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let (evs, _) = w.pump(far);
            assert!(
                evs.is_empty(),
                "файл вне наблюдения не даёт событий: {evs:?}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }

        let _ = fs::remove_file(&a);
        let _ = fs::remove_file(&b);
    }
}
