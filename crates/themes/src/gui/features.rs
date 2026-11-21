// SPDX-License-Identifier: GPL-3.0-or-later
//! Сторінка «Функції»: функції робочого столу, які надає deskd через міст оболонки
//! (поки що «Буфер обміну»), і «Подяки», згенеровані з CREDITS.md.
//!
//! Налаштування належать deskd (GSettings `io.github.OlehDzhumyk.Desk.Clipboard`): ця сторінка лише
//! прив'язує до них віджети, тож немає окремого вікна налаштувань і дублювання.

use adw::prelude::*;
use desk_proto::{CLIPBOARD_SCHEMA_ID, ClipboardProxy, SCHEMA_ID};
use gtk::{gio, glib};

use super::credits::{self, Section};

/// Звідки функція: є в яких ОС і на чому заснована (значки на сторінці функції).
struct Origin {
    has_in: &'static str,
    based_on: Option<&'static str>,
}

const CLIPBOARD_ORIGIN: Origin = Origin {
    has_in: "Є в Windows 11 і macOS",
    based_on: Some("Засновано на Clipboard Indicator (MIT)"),
};

/// Підсторінка з власним заголовком (кнопка «Назад» і назва).
fn subpage(title: &str, tag: &str, child: &impl IsA<gtk::Widget>) -> adw::NavigationPage {
    let tv = adw::ToolbarView::new();
    let hb = adw::HeaderBar::builder()
        .show_end_title_buttons(false)
        .show_start_title_buttons(false)
        .build();
    hb.add_css_class("flat");
    tv.add_top_bar(&hb);
    tv.set_content(Some(child));
    adw::NavigationPage::builder()
        .title(title)
        .tag(tag)
        .child(&tv)
        .build()
}

fn schema(id: &str) -> Option<gio::Settings> {
    let src = gio::SettingsSchemaSource::default()?;
    src.lookup(id, true)?;
    Some(gio::Settings::new(id))
}

fn badge(text: &str, extra: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("badge");
    if !extra.is_empty() {
        l.add_css_class(extra);
    }
    l
}

fn origin_group(o: &Origin) -> adw::PreferencesGroup {
    let g = adw::PreferencesGroup::new();
    let row = gtk::Box::builder()
        .orientation(gtk::Orientation::Horizontal)
        .spacing(8)
        .build();
    row.append(&badge(o.has_in, ""));
    if let Some(b) = o.based_on {
        row.append(&badge(b, "origin"));
    }
    g.add(&row);
    g
}

/// Людський вигляд скорочення: ['<Shift><Super>v'] → «Shift+Super+V».
pub fn shortcut_label(accels: &glib::StrV) -> String {
    let parts: Vec<String> = accels
        .iter()
        .filter_map(|a| {
            let (key, mods) = gtk::accelerator_parse(a.as_str())?;
            Some(gtk::accelerator_get_label(key, mods).to_string())
        })
        .collect();
    if parts.is_empty() {
        "немає".into()
    } else {
        parts.join(", ")
    }
}

fn skin_label(s: &str) -> &'static str {
    match s {
        "macos" => "панель у стилі macOS (по центру, з пошуком)",
        "windows11" => "панель Win+V у стилі Windows 11 (біля курсора)",
        _ => "просте меню (і перемикач у швидких налаштуваннях)",
    }
}

fn clear_history(toasts: &adw::ToastOverlay) {
    let toasts = toasts.clone();
    glib::spawn_future_local(async move {
        let r = gio::spawn_blocking(|| {
            zbus::block_on(async {
                let conn = zbus::Connection::session().await?;
                let p = ClipboardProxy::new(&conn).await?;
                p.clear(false).await
            })
        })
        .await;
        let msg = match r {
            Ok(Ok(())) => "Історію буфера обміну очищено".to_string(),
            Ok(Err(e)) => format!("Не вдалося очистити історію: {e}"),
            Err(_) => "Не вдалося очистити історію".to_string(),
        };
        toasts.add_toast(adw::Toast::new(&msg));
    });
}

fn count_row() -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title("Служба deskd")
        .subtitle("перевірка…")
        .build();
    let r2 = row.clone();
    glib::spawn_future_local(async move {
        let res = gio::spawn_blocking(|| {
            zbus::block_on(async {
                let conn = zbus::Connection::session().await?;
                let p = ClipboardProxy::new(&conn).await?;
                p.count().await
            })
        })
        .await;
        r2.set_subtitle(&match res {
            Ok(Ok(n)) => format!("працює; записів в історії: {n}"),
            Ok(Err(e)) => format!("недоступна: {e}"),
            Err(_) => "недоступна".into(),
        });
    });
    row
}

fn clipboard_page(toasts: &adw::ToastOverlay) -> adw::NavigationPage {
    let page = adw::PreferencesPage::builder()
        .description(
            "Історія скопійованого: тексти й зображення, закріплення, пошук. Зберігається у службі \
             deskd; панель малює міст оболонки у стилі поточної теми.",
        )
        .build();
    page.add(&origin_group(&CLIPBOARD_ORIGIN));
    let Some(s) = schema(CLIPBOARD_SCHEMA_ID) else {
        let g = adw::PreferencesGroup::builder()
            .title("Налаштування недоступні")
            .description(format!(
                "Схему GSettings {CLIPBOARD_SCHEMA_ID} не встановлено. Запустіть data/install.sh."
            ))
            .build();
        page.add(&g);
        return subpage("Буфер обміну", "clipboard", &page);
    };

    let hist = adw::PreferencesGroup::builder().title("Історія").build();
    let switch = |title: &str, subtitle: &str, key: &str| {
        let r = adw::SwitchRow::builder().title(title).subtitle(subtitle).build();
        s.bind(key, &r, "active").build();
        r
    };
    hist.add(&switch("Записувати історію", "Вимкніть, щоб функція не працювала зовсім", "enabled"));
    let size = adw::SpinRow::with_range(5.0, 500.0, 5.0);
    size.set_title("Розмір історії");
    size.set_subtitle("Скільки незакріплених записів зберігати");
    s.bind("history-size", &size, "value").build();
    hist.add(&size);
    hist.add(&switch("Зображення", "Записувати також скопійовані зображення", "images"));
    hist.add(&switch(
        "Вставляти вибране",
        "Після вибору запис одразу вставляється у вікно, де ви працювали",
        "paste-on-select",
    ));
    page.add(&hist);

    let privacy = adw::PreferencesGroup::builder()
        .title("Приватність")
        .description(
            "Паролі з менеджерів паролів (позначка x-kde-passwordManagerHint, KeePassXC, Bitwarden, \
             1Password, «Секрети» тощо) ніколи не записуються.",
        )
        .build();
    privacy.add(&switch(
        "Приватний режим",
        "Тимчасово нічого не записувати",
        "private-mode",
    ));
    privacy.add(&switch(
        "Зберігати між сеансами",
        "Текст зберігається у ~/.local/share/deskd/clipboard (доступ лише для вас)",
        "persist",
    ));
    let pimg = switch(
        "Зберігати зображення між сеансами",
        "Інакше зображення живуть лише до виходу з сеансу",
        "persist-images",
    );
    s.bind("persist", &pimg, "sensitive").get_only().build();
    privacy.add(&pimg);
    page.add(&privacy);

    let keys = adw::PreferencesGroup::builder().title("Як відкрити").build();
    let shortcut = adw::ActionRow::builder()
        .title("Клавіатурне скорочення")
        .subtitle("Задається темою (файл desk.dconf у модулі «Вигляд»)")
        .build();
    let acc = gtk::Label::new(Some(&shortcut_label(&s.strv("toggle-shortcut"))));
    acc.add_css_class("dim-label");
    shortcut.add_suffix(&acc);
    {
        let acc = acc.clone();
        s.connect_changed(Some("toggle-shortcut"), move |s, _| {
            acc.set_label(&shortcut_label(&s.strv("toggle-shortcut")));
        });
    }
    keys.add(&shortcut);
    if let Some(desk) = schema(SCHEMA_ID) {
        let skin = adw::ActionRow::builder()
            .title("Вигляд панелі")
            .subtitle(skin_label(&desk.string("skin")))
            .build();
        let skin2 = skin.clone();
        desk.connect_changed(Some("skin"), move |d, _| {
            skin2.set_subtitle(skin_label(&d.string("skin")));
        });
        // тримати об'єкт налаштувань живим разом із рядком
        skin.connect_destroy(move |_| {
            let _ = &desk;
        });
        keys.add(&skin);
    }
    keys.add(&count_row());
    page.add(&keys);

    let danger = adw::PreferencesGroup::new();
    let clear = adw::ButtonRow::builder().title("Очистити історію").build();
    clear.add_css_class("destructive-action");
    {
        let toasts = toasts.clone();
        clear.connect_activated(move |row| {
            let dlg = adw::AlertDialog::new(
                Some("Очистити історію буфера обміну?"),
                Some("Буде видалено всі записи, зокрема закріплені. Це не можна скасувати."),
            );
            dlg.add_responses(&[("cancel", "Скасувати"), ("clear", "Очистити")]);
            dlg.set_response_appearance("clear", adw::ResponseAppearance::Destructive);
            dlg.set_default_response(Some("cancel"));
            let toasts = toasts.clone();
            dlg.connect_response(None, move |_, r| {
                if r == "clear" {
                    clear_history(&toasts);
                }
            });
            dlg.present(Some(row));
        });
    }
    danger.add(&clear);
    page.add(&danger);
    page.connect_destroy(move |_| {
        let _ = &s;
    });

    subpage("Буфер обміну", "clipboard", &page)
}

fn credits_page() -> adw::NavigationPage {
    let page = adw::PreferencesPage::builder()
        .description(
            "desktop-themes-rs стоїть на плечах інших проєктів. Список узято з CREDITS.md; ліцензії — \
             ті, що вказали автори (перевіряйте файл LICENSE у їхніх сховищах).",
        )
        .build();
    for Section { title, entries } in credits::parse(credits::CREDITS_MD) {
        if entries.is_empty() {
            continue;
        }
        let g = adw::PreferencesGroup::builder().title(title.as_str()).build();
        for e in entries {
            let mut sub = Vec::new();
            if !e.used_for.is_empty() {
                sub.push(e.used_for.clone());
            }
            if !e.licence.is_empty() {
                sub.push(e.licence.clone());
            }
            let row = adw::ActionRow::builder()
                .title(glib::markup_escape_text(&e.name).as_str())
                .subtitle(glib::markup_escape_text(&sub.join(" · ")).as_str())
                .build();
            if let Some(url) = e.url.clone() {
                let b = gtk::Button::builder()
                    .icon_name("adw-external-link-symbolic")
                    .valign(gtk::Align::Center)
                    .tooltip_text(url.as_str())
                    .build();
                b.add_css_class("flat");
                b.connect_clicked(move |b| {
                    let l = gtk::UriLauncher::new(&url);
                    let win = b.root().and_downcast::<gtk::Window>();
                    l.launch(win.as_ref(), gio::Cancellable::NONE, |_| {});
                });
                row.add_suffix(&b);
            }
            g.add(&row);
        }
        page.add(&g);
    }
    subpage("Подяки", "credits", &page)
}

/// Сторінка «Функції» (навігація: список функцій → сторінка функції; «Подяки»).
pub fn build(toasts: &adw::ToastOverlay, open: Option<&str>) -> adw::NavigationView {
    let nav = adw::NavigationView::new();
    let root = adw::PreferencesPage::builder()
        .description(
            "Функції, які є в Windows 11 і macOS, для GNOME. Їх виконує служба deskd, а вигляд \
             підлаштовується під поточну тему.",
        )
        .build();
    let g = adw::PreferencesGroup::builder().title("Функції").build();
    let clip = adw::ActionRow::builder()
        .title("Буфер обміну")
        .subtitle("Історія скопійованого: Win+V у Windows 11, Shift+Super+V у macOS")
        .activatable(true)
        .build();
    clip.add_prefix(&gtk::Image::from_icon_name("edit-paste-symbolic"));
    clip.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    g.add(&clip);
    root.add(&g);
    let about = adw::PreferencesGroup::builder().title("Про проєкт").build();
    let cr = adw::ActionRow::builder()
        .title("Подяки")
        .subtitle("Проєкти, на яких засновано теми й функції, та їхні ліцензії")
        .activatable(true)
        .build();
    cr.add_prefix(&gtk::Image::from_icon_name("emblem-favorite-symbolic"));
    cr.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
    about.add(&cr);
    root.add(&about);
    nav.add(
        &adw::NavigationPage::builder()
            .title("Функції")
            .tag("features")
            .child(&root)
            .build(),
    );
    {
        let nav2 = nav.clone();
        let toasts = toasts.clone();
        clip.connect_activated(move |_| nav2.push(&clipboard_page(&toasts)));
    }
    {
        let nav2 = nav.clone();
        cr.connect_activated(move |_| nav2.push(&credits_page()));
    }
    match open {
        Some("clipboard") => nav.push(&clipboard_page(toasts)),
        Some("credits") => nav.push(&credits_page()),
        _ => {}
    }
    nav
}
