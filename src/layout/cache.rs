//! Кэш подготовленных блоков: гальке́й и метрики отрисовки.
//! Ограничен по числу записей; вытеснение — по расстоянию от текущего блока
//! (при скролле ближние блоки нужны снова, дальние — не скоро).

use std::collections::HashMap;

use super::measure::Prepared;

/// Максимум хранимых блоков (сотни видимых + запас на резкий скролл).
const CAP: usize = 1536;
/// При переполнении выбрасываем блоки дальше этого расстояния от `center`.
const KEEP_SPAN: usize = 1024;

#[derive(Default)]
pub struct BlockCache(HashMap<usize, Prepared>);

impl BlockCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn get(&self, i: usize) -> Option<&Prepared> {
        self.0.get(&i)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut Prepared> {
        self.0.get_mut(&i)
    }

    pub fn put(&mut self, i: usize, p: Prepared) {
        self.0.insert(i, p);
    }

    /// Вытеснение дальних блоков, когда кэш переполнен.
    pub fn evict_around(&mut self, center: usize) {
        if self.0.len() > CAP {
            self.0.retain(|&k, _| k.abs_diff(center) <= KEEP_SPAN);
        }
    }
}
