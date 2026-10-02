//! Рабочие столы и фокус.
//!
//! Здесь живёт вся память среды о том, какое окно где находится и какое из них
//! активно. Модуль тоже чистый: ни одного вызова в Wayland, поэтому поведение
//! фокуса проверяется тестами, а не запуском сеанса и кликаньем мышью.

use hype_anim::Rect;
use hype_config::Direction;

use crate::layout::focus_target;

/// Один рабочий стол.
#[derive(Debug, Clone, PartialEq)]
pub struct Workspace {
    pub index: u8,
    /// Окна в порядке раскладки: первое — главное.
    pub windows: Vec<u64>,
}

impl Workspace {
    fn new(index: u8) -> Self {
        Self {
            index,
            windows: Vec::new(),
        }
    }
}

/// Все рабочие столы сеанса.
#[derive(Debug, Clone, PartialEq)]
pub struct Workspaces {
    spaces: Vec<Workspace>,
    active: u8,
    focused: Option<u64>,
}

impl Workspaces {
    /// Создаёт `count` рабочих столов, пронумерованных с единицы.
    ///
    /// Ноль столов невозможен: среде негде было бы показывать окна.
    pub fn new(count: u8) -> Self {
        let count = count.max(1);
        Self {
            spaces: (1..=count).map(Workspace::new).collect(),
            active: 1,
            focused: None,
        }
    }

    pub fn count(&self) -> u8 {
        self.spaces.len() as u8
    }

    pub fn active_index(&self) -> u8 {
        self.active
    }

    pub fn focused(&self) -> Option<u64> {
        self.focused
    }

    pub fn active(&self) -> &Workspace {
        &self.spaces[self.active as usize - 1]
    }

    fn active_mut(&mut self) -> &mut Workspace {
        let index = self.active as usize - 1;
        &mut self.spaces[index]
    }

    pub fn get(&self, index: u8) -> Option<&Workspace> {
        self.spaces.get(index.checked_sub(1)? as usize)
    }

    pub fn all(&self) -> &[Workspace] {
        &self.spaces
    }

    /// Окна текущего рабочего стола.
    pub fn visible_windows(&self) -> &[u64] {
        &self.active().windows
    }

    /// На каком рабочем столе находится окно.
    pub fn workspace_of(&self, window: u64) -> Option<u8> {
        self.spaces
            .iter()
            .find(|s| s.windows.contains(&window))
            .map(|s| s.index)
    }

    /// Добавляет новое окно на текущий рабочий стол и отдаёт ему фокус.
    ///
    /// Окно встаёт в начало списка: только что открытое приложение должно
    /// оказаться главным, а не уехать в хвост стопки.
    pub fn add_window(&mut self, window: u64) {
        self.active_mut().windows.insert(0, window);
        self.focused = Some(window);
    }

    /// Убирает окно отовсюду.
    ///
    /// Если оно было в фокусе, фокус переходит соседу по тому же столу —
    /// оставлять пользователя без фокуса после закрытия окна нельзя.
    pub fn remove_window(&mut self, window: u64) {
        let was_focused = self.focused == Some(window);
        let mut neighbour = None;

        for space in &mut self.spaces {
            if let Some(position) = space.windows.iter().position(|w| *w == window) {
                space.windows.remove(position);
                if space.index == self.active {
                    neighbour = space
                        .windows
                        .get(position)
                        .or_else(|| space.windows.last())
                        .copied();
                }
            }
        }

        if was_focused {
            self.focused = neighbour;
        }
    }

    /// Переходит на другой рабочий стол.
    ///
    /// Возвращает `false`, если такого стола нет. Фокус переезжает на первое
    /// окно нового стола, а пустой стол оставляет фокус ни на чём.
    pub fn activate(&mut self, index: u8) -> bool {
        if index == 0 || index > self.count() {
            return false;
        }
        self.active = index;
        self.focused = self.active().windows.first().copied();
        true
    }

    /// Следующий рабочий стол по кругу.
    pub fn activate_next(&mut self) {
        let next = if self.active >= self.count() {
            1
        } else {
            self.active + 1
        };
        self.activate(next);
    }

    /// Предыдущий рабочий стол по кругу.
    pub fn activate_prev(&mut self) {
        let prev = if self.active <= 1 {
            self.count()
        } else {
            self.active - 1
        };
        self.activate(prev);
    }

    /// Переносит окно в фокусе на другой стол и переходит туда следом.
    ///
    /// Идти следом — намеренное решение: пользователь, отправивший окно на
    /// другой стол, почти всегда хочет продолжить работу с ним.
    pub fn move_focused_to(&mut self, index: u8) -> bool {
        let Some(window) = self.focused else {
            return false;
        };
        if index == 0 || index > self.count() || index == self.active {
            return false;
        }

        self.remove_window(window);
        self.spaces[index as usize - 1].windows.insert(0, window);
        self.activate(index);
        self.focused = Some(window);
        true
    }

    /// Ставит фокус на конкретное окно, если оно видимо.
    pub fn focus_window(&mut self, window: u64) -> bool {
        if !self.active().windows.contains(&window) {
            return false;
        }
        self.focused = Some(window);
        true
    }

    /// Переводит фокус на соседнее окно по геометрии.
    ///
    /// `geometry` — актуальные прямоугольники видимых окон. Раскладка живёт
    /// отдельно, поэтому переход фокуса получает её результат снаружи.
    pub fn focus_direction(&mut self, direction: Direction, geometry: &[(u64, Rect)]) -> bool {
        let Some(current) = self.focused else {
            // Фокуса нет — берём любое видимое окно, это лучше, чем ничего.
            if let Some(first) = self.active().windows.first().copied() {
                self.focused = Some(first);
                return true;
            }
            return false;
        };

        let Some(from) = geometry
            .iter()
            .find(|(id, _)| *id == current)
            .map(|(_, r)| *r)
        else {
            return false;
        };

        let candidates: Vec<(u64, Rect)> = geometry
            .iter()
            .filter(|(id, _)| *id != current && self.active().windows.contains(id))
            .copied()
            .collect();

        match focus_target(from, &candidates, direction) {
            Some(target) => {
                self.focused = Some(target);
                true
            }
            None => false,
        }
    }

    /// Меняет местами окно в фокусе и его соседа в заданном направлении.
    ///
    /// Так окно переставляется внутри плитки: раскладка считается по порядку
    /// списка, поэтому перестановка в списке и есть перемещение окна.
    pub fn move_window(&mut self, direction: Direction, geometry: &[(u64, Rect)]) -> bool {
        let Some(current) = self.focused else {
            return false;
        };
        let Some(from) = geometry
            .iter()
            .find(|(id, _)| *id == current)
            .map(|(_, r)| *r)
        else {
            return false;
        };

        let candidates: Vec<(u64, Rect)> = geometry
            .iter()
            .filter(|(id, _)| *id != current && self.active().windows.contains(id))
            .copied()
            .collect();

        let Some(target) = focus_target(from, &candidates, direction) else {
            return false;
        };

        let windows = &mut self.active_mut().windows;
        let (Some(a), Some(b)) = (
            windows.iter().position(|w| *w == current),
            windows.iter().position(|w| *w == target),
        ) else {
            return false;
        };
        windows.swap(a, b);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid() -> Vec<(u64, Rect)> {
        // Две колонки по два окна: слева 1 и 2, справа 3 и 4.
        vec![
            (1, Rect::new(0.0, 0.0, 100.0, 100.0)),
            (2, Rect::new(0.0, 200.0, 100.0, 100.0)),
            (3, Rect::new(200.0, 0.0, 100.0, 100.0)),
            (4, Rect::new(200.0, 200.0, 100.0, 100.0)),
        ]
    }

    fn with_windows(ids: &[u64]) -> Workspaces {
        let mut spaces = Workspaces::new(4);
        // add_window вставляет в начало, поэтому идём с конца — получаем
        // список ровно в переданном порядке.
        for id in ids.iter().rev() {
            spaces.add_window(*id);
        }
        spaces.focused = ids.first().copied();
        spaces
    }

    #[test]
    fn a_fresh_session_starts_on_the_first_workspace() {
        let spaces = Workspaces::new(9);
        assert_eq!(spaces.count(), 9);
        assert_eq!(spaces.active_index(), 1);
        assert_eq!(spaces.focused(), None);
        assert!(spaces.visible_windows().is_empty());
    }

    #[test]
    fn there_is_always_at_least_one_workspace() {
        assert_eq!(Workspaces::new(0).count(), 1);
    }

    #[test]
    fn a_new_window_becomes_the_master_and_takes_focus() {
        let mut spaces = Workspaces::new(4);
        spaces.add_window(1);
        spaces.add_window(2);
        assert_eq!(spaces.visible_windows(), &[2, 1]);
        assert_eq!(spaces.focused(), Some(2));
    }

    #[test]
    fn closing_a_window_passes_focus_to_a_neighbour() {
        let mut spaces = with_windows(&[1, 2, 3]);
        assert_eq!(spaces.focused(), Some(1));
        spaces.remove_window(1);
        assert_eq!(spaces.visible_windows(), &[2, 3]);
        assert_eq!(spaces.focused(), Some(2));
    }

    #[test]
    fn closing_the_last_window_leaves_no_focus() {
        let mut spaces = with_windows(&[1]);
        spaces.remove_window(1);
        assert_eq!(spaces.focused(), None);
        assert!(spaces.visible_windows().is_empty());
    }

    #[test]
    fn closing_an_unfocused_window_does_not_move_focus() {
        let mut spaces = with_windows(&[1, 2, 3]);
        spaces.remove_window(3);
        assert_eq!(spaces.focused(), Some(1));
    }

    #[test]
    fn closing_the_last_in_the_list_falls_back_to_the_previous_one() {
        let mut spaces = with_windows(&[1, 2, 3]);
        spaces.focus_window(3);
        spaces.remove_window(3);
        assert_eq!(spaces.focused(), Some(2));
    }

    #[test]
    fn switching_workspaces_hides_the_other_windows() {
        let mut spaces = with_windows(&[1, 2]);
        assert!(spaces.activate(2));
        assert!(spaces.visible_windows().is_empty());
        assert_eq!(spaces.focused(), None);

        assert!(spaces.activate(1));
        assert_eq!(spaces.visible_windows(), &[1, 2]);
        assert_eq!(spaces.focused(), Some(1));
    }

    #[test]
    fn a_workspace_that_does_not_exist_is_refused() {
        let mut spaces = Workspaces::new(4);
        assert!(!spaces.activate(0));
        assert!(!spaces.activate(5));
        assert_eq!(spaces.active_index(), 1);
    }

    #[test]
    fn next_and_previous_wrap_around() {
        let mut spaces = Workspaces::new(3);
        spaces.activate_prev();
        assert_eq!(spaces.active_index(), 3);
        spaces.activate_next();
        assert_eq!(spaces.active_index(), 1);
    }

    #[test]
    fn moving_a_window_takes_it_to_the_other_workspace_and_follows_it() {
        let mut spaces = with_windows(&[1, 2]);
        assert!(spaces.move_focused_to(3));

        assert_eq!(spaces.active_index(), 3);
        assert_eq!(spaces.visible_windows(), &[1]);
        assert_eq!(spaces.focused(), Some(1));
        assert_eq!(spaces.get(1).unwrap().windows, vec![2]);
        assert_eq!(spaces.workspace_of(1), Some(3));
    }

    #[test]
    fn moving_a_window_to_its_own_workspace_does_nothing() {
        let mut spaces = with_windows(&[1]);
        assert!(!spaces.move_focused_to(1));
        assert_eq!(spaces.visible_windows(), &[1]);
    }

    #[test]
    fn moving_without_a_focused_window_does_nothing() {
        let mut spaces = Workspaces::new(4);
        assert!(!spaces.move_focused_to(2));
        assert_eq!(spaces.active_index(), 1);
    }

    #[test]
    fn focus_follows_the_geometry() {
        let mut spaces = with_windows(&[1, 2, 3, 4]);
        spaces.focus_window(1);

        assert!(spaces.focus_direction(Direction::Right, &grid()));
        assert_eq!(spaces.focused(), Some(3));

        assert!(spaces.focus_direction(Direction::Down, &grid()));
        assert_eq!(spaces.focused(), Some(4));

        assert!(spaces.focus_direction(Direction::Left, &grid()));
        assert_eq!(spaces.focused(), Some(2));
    }

    #[test]
    fn focus_stays_put_when_there_is_no_window_that_way() {
        let mut spaces = with_windows(&[1, 2, 3, 4]);
        spaces.focus_window(1);
        assert!(!spaces.focus_direction(Direction::Up, &grid()));
        assert_eq!(spaces.focused(), Some(1));
    }

    #[test]
    fn focus_never_crosses_to_a_hidden_workspace() {
        let mut spaces = with_windows(&[1, 2]);
        spaces.move_focused_to(2);
        spaces.activate(1);
        spaces.focus_window(2);

        // Окно 1 уехало на другой стол — его геометрия ещё в списке, но
        // фокусу там делать нечего.
        let geometry = vec![
            (2, Rect::new(0.0, 0.0, 100.0, 100.0)),
            (1, Rect::new(200.0, 0.0, 100.0, 100.0)),
        ];
        assert!(!spaces.focus_direction(Direction::Right, &geometry));
        assert_eq!(spaces.focused(), Some(2));
    }

    #[test]
    fn focus_recovers_when_nothing_is_focused() {
        let mut spaces = with_windows(&[1, 2]);
        spaces.focused = None;
        assert!(spaces.focus_direction(Direction::Right, &grid()));
        assert_eq!(spaces.focused(), Some(1));
    }

    #[test]
    fn moving_a_window_swaps_it_with_its_neighbour() {
        let mut spaces = with_windows(&[1, 2, 3, 4]);
        spaces.focus_window(1);

        assert!(spaces.move_window(Direction::Right, &grid()));
        assert_eq!(spaces.visible_windows(), &[3, 2, 1, 4]);
        // Фокус остаётся на том же окне, оно просто переехало.
        assert_eq!(spaces.focused(), Some(1));
    }

    #[test]
    fn moving_a_window_nowhere_leaves_the_order_alone() {
        let mut spaces = with_windows(&[1, 2, 3, 4]);
        spaces.focus_window(1);
        assert!(!spaces.move_window(Direction::Up, &grid()));
        assert_eq!(spaces.visible_windows(), &[1, 2, 3, 4]);
    }

    #[test]
    fn focusing_a_window_from_another_workspace_is_refused() {
        let mut spaces = with_windows(&[1, 2]);
        spaces.move_focused_to(2);
        spaces.activate(1);
        assert!(!spaces.focus_window(1));
        assert_eq!(spaces.focused(), Some(2));
    }
}
