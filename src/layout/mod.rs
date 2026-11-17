//! Состояние раскладки одной вкладки: высоты блоков (измеренные + оценки),
//! префикс-суммы позиций, кэш подготовленных блоков.
//! Блоки не знают про экран; экран не трогает блоки напрямую — только через здесь.

pub mod cache;
pub mod measure;

use crate::i18n::Lang;
use crate::md::blocks::Doc;
use crate::settings::model::TextAlign;
use crate::view::themes::Typo;

use cache::BlockCache;

/// Параметры раскладки: сменились — все высоты сбрасываются в оценки
/// (файл при этом не перечитывается).
#[derive(Clone, Copy, PartialEq)]
pub struct LayoutKey {
    pub typo: Typo,
    /// Ширина текстовой колонки.
    pub width: f32,
    pub dark: bool,
    /// Язык интерфейса (перевешиваются подписи alert-цитат).
    pub lang: Lang,
    /// Нумерация строк кода (меняет ширину гуттера).
    pub code_nums: bool,
    /// Горизонтальный скролл в таблицах (меняет ширины колонок и высоту).
    pub table_scroll: bool,
    /// Выравнивание текста (строки в галлеях раскладываются иначе).
    pub text_align: TextAlign,
}

/// Высоты блоков + префикс-суммы + кэш готовых к отрисовке блоков.
pub struct LayoutState {
    key: Option<LayoutKey>,
    /// Высота каждого блока (измеренная или оценка).
    heights: Vec<f32>,
    measured: Vec<bool>,
    /// offsets[i] — верх блока i; len = blocks+1. Пересобирается по dirty.
    offsets: Vec<f32>,
    cache: BlockCache,
    /// Якорь прошлого кадра: (первый видимый блок, его верх).
    anchor: Option<(usize, f32)>,
    /// Накопленная коррекция скролла (высоты выше вьюпорта изменились).
    pending_scroll: f32,
    dirty: bool,
    /// Растёт при любом изменении высот/ключа (инвалидация кэша миникарты).
    version: u64,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            key: None,
            heights: Vec::new(),
            measured: Vec::new(),
            offsets: Vec::new(),
            cache: BlockCache::new(),
            anchor: None,
            pending_scroll: 0.0,
            dirty: false,
            version: 0,
        }
    }
}

impl LayoutState {
    /// Подгоняет состояние под документ и ключ раскладки (типографика/ширина/тема).
    pub fn ensure(&mut self, doc: &Doc, key: LayoutKey) {
        if self.key != Some(key) {
            self.key = Some(key);
            self.heights.clear();
            self.measured.clear();
            self.offsets.clear();
            self.cache.clear();
            self.anchor = None;
            self.pending_scroll = 0.0;
            self.dirty = false;
            self.version += 1;
        }
        let n = doc.blocks.len();
        if self.heights.len() < n {
            let source = doc.source.as_ref();
            for block in &doc.blocks[self.heights.len()..] {
                self.heights
                    .push(measure::estimate(block, &key.typo, key.width, source));
                self.measured.push(false);
            }
            self.dirty = true;
            self.version += 1;
        }
        if self.dirty || self.offsets.len() != n + 1 {
            self.rebuild_offsets();
            self.dirty = false;
        }
    }

    fn rebuild_offsets(&mut self) {
        let n = self.heights.len();
        self.offsets.clear();
        self.offsets.reserve(n + 1);
        self.offsets.push(0.0);
        let mut acc = 0.0;
        for &h in &self.heights {
            acc += h;
            self.offsets.push(acc);
        }
    }

    pub fn total_height(&self) -> f32 {
        self.offsets.last().copied().unwrap_or(0.0)
    }

    /// Версия раскладки: меняется при смене ключа/росте/перемерах высот.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Был ли хотя бы один ensure (ключ раскладки установлен): высоты
    /// существуют хотя бы как оценки. Нужно восстановлению позиции.
    pub fn is_ready(&self) -> bool {
        self.key.is_some()
    }

    pub fn height(&self, i: usize) -> f32 {
        self.heights[i.min(self.heights.len().saturating_sub(1))]
    }

    /// Верх блока i в координатах контента. Пустая раскладка (ensure ещё не
    /// звался — панели навигации читают до первого кадра doc_view) → 0.
    pub fn offset(&self, i: usize) -> f32 {
        self.offsets
            .get(i.min(self.offsets.len().saturating_sub(1)))
            .copied()
            .unwrap_or(0.0)
    }

    /// Число блоков, для которых есть высоты (после ensure; при стриминге
    /// может отставать от doc.blocks.len() до ближайшего кадра doc_view).
    pub fn blocks_len(&self) -> usize {
        self.heights.len()
    }

    /// Диапазон блоков, пересекающих [top, bottom) в координатах контента.
    pub fn visible_range(&self, top: f32, bottom: f32) -> std::ops::Range<usize> {
        let n = self.heights.len();
        if n == 0 || bottom <= 0.0 || top >= self.total_height() {
            return 0..0;
        }
        let i0 = self.offsets.partition_point(|&o| o <= top).saturating_sub(1);
        let i1 = self.offsets.partition_point(|&o| o < bottom).min(n);
        i0..i1.max(i0)
    }

    /// Запоминает измеренную высоту; при заметном изменении помечает суммы грязными.
    pub fn set_measured(&mut self, i: usize, h: f32) {
        self.measured[i] = true;
        if (self.heights[i] - h).abs() > 0.01 {
            self.heights[i] = h;
            self.dirty = true;
            self.version += 1;
        }
    }

    pub fn prepared(&self, i: usize) -> Option<&measure::Prepared> {
        self.cache.get(i)
    }

    pub fn prepared_mut(&mut self, i: usize) -> Option<&mut measure::Prepared> {
        self.cache.get_mut(i)
    }

    pub fn put_prepared(&mut self, i: usize, p: measure::Prepared) {
        self.cache.put(i, p);
        self.cache.evict_around(i);
    }

    /// Конец кадра: пересборка сумм и компенсация «прилипания» — если верх
    /// первого видимого блока сдвинулся (изменились высоты выше), скролл
    /// поправится на ту же величину в следующем кадре.
    /// Возвращает true, если нужен повторный кадр.
    pub fn finish_frame(&mut self, i0: usize) -> bool {
        let mut changed = false;
        if self.dirty {
            self.rebuild_offsets();
            self.dirty = false;
            changed = true;
        }
        if let Some((pi, po)) = self.anchor {
            if pi == i0 && i0 < self.offsets.len() {
                let d = self.offsets[i0] - po;
                if d.abs() > 0.05 {
                    self.pending_scroll += d;
                    changed = true;
                }
            }
        }
        if i0 < self.offsets.len() {
            self.anchor = Some((i0, self.offsets[i0]));
        }
        changed
    }

    /// Очередная коррекция скролла (применяется один раз).
    pub fn take_pending_scroll(&mut self) -> Option<f32> {
        if self.pending_scroll.abs() > 0.05 {
            let d = self.pending_scroll;
            self.pending_scroll = 0.0;
            Some(d)
        } else {
            self.pending_scroll = 0.0;
            None
        }
    }
}
