// SPDX-License-Identifier: GPL-3.0-or-later
//! Доступність: назви й описи віджетів для читачів екрана (властивості GtkAccessible).
//! Рядки libadwaita (заголовок/підзаголовок) озвучуються самі; ці помічники — для кнопок лише зі
//! значком, карток, полів без підпису тощо.

use gtk::prelude::*;

/// Назва (accessible label), яку озвучує читач екрана.
pub fn label(w: &impl IsA<gtk::Accessible>, text: &str) {
    w.update_property(&[gtk::accessible::Property::Label(text)]);
}

/// Опис (accessible description).
pub fn description(w: &impl IsA<gtk::Accessible>, text: &str) {
    w.update_property(&[gtk::accessible::Property::Description(text)]);
}
