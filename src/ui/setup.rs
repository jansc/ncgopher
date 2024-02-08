use crate::bookmarks::Bookmark;
use crate::controller::{Controller, Direction};
use crate::gophermap::{GopherMapEntry, ItemType};
use crate::history::HistoryEntry;
use crate::settings::{default_keybindings, KeyBindings};
use crate::ui::{dialogs, layout::Layout, statusbar::StatusBar};
use crate::SETTINGS;
use cursive::{
    event::Key,
    menu::Tree,
    view::{Nameable, Resizable, Scrollable},
    views::{Dialog, NamedView, OnEventView, ResizedView, ScrollView, SelectView, ViewRef},
    Cursive, View,
};
use url::Url;

fn render_help_text() -> String {
    let keybindings = SETTINGS
        .read()
        .expect("Could not get keybindings!")
        .config
        .keybindings
        .clone()
        .unwrap();

    format!(
        r#"
|------------+--------------------------------|
| Key        | Command                        |
|------------+--------------------------------|
| Arrow keys | Move around in text            |
| Enter      | Open the link under the cursor |
| Esc        | Go to menubar                  |
| Space      | Scroll down one page           |
| {}          | Open new URL                   |
| {}          | Edit current URL               |
| {}          | Navigate back                  |
| {}          | Close application              |
| {}          | Save current page              |
| {}          | Reload current page            |
| {}          | Show link under cursor         |
| {}          | Add bookmark for current page  |
| {}          | Go to next link                |
| {}          | Go to previous link            |
| {}          | Move one line down             |
| {}          | Move one line up               |
| {}          | Search in text                 |
| {}          | Move to next search result     |
| {}          | Move to previous search result |
| {}          | Display this help text         |
|------------+--------------------------------|"#,
        keybindings.open_new_url,
        keybindings.edit_current_url,
        keybindings.navigate_back,
        keybindings.close,
        keybindings.save_page,
        keybindings.reload_page,
        keybindings.show_link,
        keybindings.add_bookmark,
        keybindings.next_link,
        keybindings.previous_link,
        keybindings.move_down,
        keybindings.move_up,
        keybindings.search_in_text,
        keybindings.next_search_result,
        keybindings.previous_search_result,
        keybindings.show_help,
    )
}

pub fn setup(app: &mut Cursive) {
    trace!("ui::setup");
    setup_keys(app);
    setup_menu(app);
    setup_ui(app);
}

/// Register global keys.
fn setup_keys(app: &mut Cursive) {
    app.set_autohide_menu(false);

    let keybindings = SETTINGS
        .read()
        .expect("Could not get keybindings!")
        .config
        .keybindings
        .clone()
        .unwrap_or(default_keybindings());

    app.add_global_callback(Key::Esc, |app| {
        app.call_on_name("main", |v: &mut Layout| v.clear_search())
            .expect("main layout missing");
        app.select_menubar()
    });
    app.add_global_callback(keybindings.close, Cursive::quit);
    app.add_global_callback(keybindings.open_new_url, dialogs::open_url);
    app.add_global_callback(keybindings.edit_current_url, dialogs::open_current_url);

    app.add_global_callback(keybindings.navigate_back, |app| {
        // step back history
        app.user_data::<Controller>()
            .expect("controller missing")
            .navigate_back();
    });
    app.add_global_callback(keybindings.reload_page, |app| {
        // reload the current page
        let index = Controller::get_selected_item_index(app);
        let controller = app.user_data::<Controller>().expect("controller missing");
        let current_url = controller.current_url.lock().unwrap().clone();
        controller.open_url(current_url, false, index);
    });
    app.add_global_callback(keybindings.save_page, dialogs::save_as);
    app.add_global_callback(keybindings.show_link, |app| {
        // show info about currently selected line
        let current_view = app
            .call_on_name("main", |v: &mut Layout| v.get_current_view())
            .expect("main layout missing");

        match current_view.as_str() {
            "content" => {
                let view: ViewRef<SelectView<GopherMapEntry>> =
                    app.find_name("content").expect("View content missing");
                let cur = view.selected_id().unwrap_or(0);
                if let Some((_, item)) = view.get_item(cur) {
                    match item.item_type {
                        ItemType::Html => {
                            let mut url = item.url.to_string();
                            if url.starts_with("URL:") {
                                url.replace_range(..3, "");
                            }
                            app.user_data::<Controller>()
                                .expect("controller missing")
                                .set_message(&format!("URL '{}'", url));
                        }
                        ItemType::Inline => (),
                        _ => app
                            .user_data::<Controller>()
                            .expect("controller missing")
                            .set_message(&format!("URL '{}'", item.url)),
                    }
                };
            }
            "gemini_content" => {
                let view: ViewRef<SelectView<Option<Url>>> = app
                    .find_name("gemini_content")
                    .expect("View gemini missing");
                let cur = view.selected_id().unwrap_or(0);
                if let Some((_, Some(url))) = view.get_item(cur) {
                    app.user_data::<Controller>()
                        .expect("controller missing")
                        .set_message(&format!("URL '{}'", url));
                }
            }
            other => unreachable!("unknown view {} in main layout", other),
        }
    });
    app.add_global_callback(keybindings.move_down, |app| {
        // go to next line
        move_selection(app, Direction::Next);
    });
    app.add_global_callback(keybindings.move_up, |app| {
        // go to previous line
        move_selection(app, Direction::Previous);
    });
    app.add_global_callback(keybindings.next_link /*Key::Tab*/, |app| {
        // go to next link
        move_to_link(app, Direction::Next);
    });
    app.add_global_callback(
        keybindings.previous_link, /*Event::Shift(Key::Tab)*/
        |app| {
            // go to previous link
            move_to_link(app, Direction::Previous);
        },
    );
    app.add_global_callback(keybindings.add_bookmark, dialogs::add_bookmark_current_url);
    app.add_global_callback(keybindings.show_help, |s| {
        s.add_layer(Dialog::info(render_help_text().as_str()))
    });
    app.add_global_callback(keybindings.search_in_text, move |app| {
        app.call_on_name("main", |v: &mut Layout| v.enable_search())
            .expect("main layout missing");
    });
    app.add_global_callback(keybindings.next_search_result, |app| {
        let controller = app.user_data::<Controller>().expect("controller missing");
        let hits = controller.current_search_results.clone();
        if let Some(content) = app.find_name::<SelectView<GopherMapEntry>>("content") {
            let scroll_view = app
                .find_name::<ScrollView<ResizedView<NamedView<SelectView<GopherMapEntry>>>>>(
                    "content_scroll",
                )
                .expect("gopher scroll view missing");
            move_to_next_item(content, scroll_view, Direction::Next, hits);
        } else if let Some(content) = app.find_name::<SelectView<Option<Url>>>("gemini_content") {
            let scroll_view = app
                .find_name::<ScrollView<ResizedView<NamedView<SelectView<Option<Url>>>>>>(
                    "gemini_content_scroll",
                )
                .expect("gemini scroll view missing");
            move_to_next_item(content, scroll_view, Direction::Next, hits);
        } else {
            unreachable!("view content and gemini_content missing");
        }
    });
    app.add_global_callback(keybindings.previous_search_result, |app| {
        let controller = app.user_data::<Controller>().expect("controller missing");
        let hits = controller.current_search_results.clone();
        if let Some(content) = app.find_name::<SelectView<GopherMapEntry>>("content") {
            let scroll_view = app
                .find_name::<ScrollView<ResizedView<NamedView<SelectView<GopherMapEntry>>>>>(
                    "content_scroll",
                )
                .expect("gopher scroll view missing");
            move_to_next_item(content, scroll_view, Direction::Previous, hits);
        } else if let Some(content) = app.find_name::<SelectView<Option<Url>>>("gemini_content") {
            let scroll_view = app
                .find_name::<ScrollView<ResizedView<NamedView<SelectView<Option<Url>>>>>>(
                    "gemini_content_scroll",
                )
                .expect("gemini scroll view missing");
            move_to_next_item(content, scroll_view, Direction::Previous, hits);
        } else {
            unreachable!("view content and gemini_content missing");
        }
    });
}

fn setup_menu(app: &mut Cursive) {
    let menubar = app.menubar();
    menubar.add_subtree(
        "File",
        Tree::new()
            .leaf("Open URL...", dialogs::open_url)
            .delimiter()
            .leaf("Save page as...", dialogs::save_as)
            .leaf("Settings...", dialogs::settings)
            .delimiter()
            .leaf("Quit", Cursive::quit),
    );
    menubar.add_subtree(
        "History",
        Tree::new()
            .leaf("Show all history...", dialogs::edit_history)
            .leaf("Clear history", |app| {
                app.user_data::<Controller>()
                    .expect("controller missing")
                    .clear_history();
            })
            .delimiter(),
    );
    menubar.add_subtree(
        "Bookmarks",
        Tree::new()
            .leaf("Edit...", dialogs::edit_bookmarks)
            .leaf("Add bookmark", dialogs::add_bookmark_current_url)
            .delimiter(),
    );
    menubar.add_subtree(
        "Identities",
        Tree::new()
            .leaf("New identity...", |app| {
                dialogs::add_client_certificate(app, None);
            })
            .leaf("Manage identities...", dialogs::manage_client_certificates),
    );
    menubar.add_subtree(
        "Help",
        Tree::new()
            .subtree(
                "Help",
                Tree::new()
                    .leaf("Keys", |s| {
                        s.add_layer(Dialog::info(render_help_text().as_str()))
                    })
                    .leaf("Extended", |app| {
                        app.user_data::<Controller>()
                            .expect("controller missing")
                            .open_url(Url::parse("about:help").unwrap(), false, 0);
                    })
                    .leaf("Release notes", |app| {
                        app.user_data::<Controller>()
                            .expect("controller missing")
                            .open_url(Url::parse("about:release-notes").unwrap(), false, 0);
                    }),
            )
            .leaf("About", |s| {
                s.add_layer(Dialog::info(format!(
                    "                      ncgopher v{:<15}\n\
                     \u{20}     A Gopher and Gemini client for the modern internet\n\
                     \u{20}              (c) 2019-2022 The ncgopher Authors\n\
                     \u{20}\n\
                     \u{20}  Originally developed by Jan Schreiber <jan@mecinus.com>\n\
                     \u{20}                     gopher://jan.bio\n\
                     \u{20}                     gemini://jan.bio",
                    env!("CARGO_PKG_VERSION")
                )))
            }),
    );
}

/// Set up the user interface
fn setup_ui(app: &mut Cursive) {
    info!("setup_ui");

    // Create gophermap content view
    let view: SelectView<GopherMapEntry> = SelectView::new();
    let scrollable = view
        .with_name("content")
        .full_width()
        .scrollable()
        .with_name("content_scroll");
    let event_view = OnEventView::new(scrollable).on_event(' ', |app| {
        app.call_on_name(
            "content_scroll",
            |s: &mut ScrollView<ResizedView<NamedView<SelectView<GopherMapEntry>>>>| {
                let rect = s.content_viewport();
                let bl = rect.bottom_left();
                s.set_offset(bl);
            },
        );
    });

    // Create gemini content view
    let view: SelectView<Option<Url>> = SelectView::new();
    let scrollable = view
        .with_name("gemini_content")
        .full_width()
        .scrollable()
        .with_name("gemini_content_scroll");
    let gemini_event_view = OnEventView::new(scrollable).on_event(' ', |app| {
        app.call_on_name(
            "gemini_content_scroll",
            |s: &mut ScrollView<ResizedView<NamedView<SelectView<Option<Url>>>>>| {
                let rect = s.content_viewport();
                let bl = rect.bottom_left();
                s.set_offset(bl);
            },
        );
    });
    let status = StatusBar::new().with_name("statusbar");
    let mut layout = Layout::new(status /*, theme*/)
        .view("content", event_view, "Gophermap")
        .view("gemini_content", gemini_event_view, "Gemini");
    layout.set_view("content");
    app.add_fullscreen_layer(layout.with_name("main"));

    app.call_on_name("main", |v: &mut Layout| {
        v.search.set_on_edit(move |app, cmd, _| {
            app.call_on_name("main", |v: &mut Layout| {
                if cmd.is_empty() {
                    v.clear_search();
                }
            });
        });
        v.search.set_on_submit(move |app, search_str| {
            app.call_on_name("main", |v: &mut Layout| {
                v.clear_search();
            });
            app.user_data::<Controller>()
                .expect("controller missing")
                .search(search_str[1..].to_string());
        });
    })
    .expect("main layout missing");
}

pub fn setup_bookmark_menu(app: &mut Cursive, bookmarks: &Vec<Bookmark>) {
    // Add bookmarks to bookmark menu on startup
    info!("Adding existing bookmarks to menu");
    let menutree = app
        .menubar()
        .find_subtree("Bookmarks")
        .expect("bookmarks menu missing");
    for entry in bookmarks {
        let url = entry.url.clone();
        menutree.insert_leaf(3, &entry.title, move |app| {
            app.user_data::<Controller>()
                .expect("controller missing")
                .open_url(url.clone(), true, 0);
        });
    }
}

pub fn setup_history_menu(app: &mut Cursive, entries: &Vec<HistoryEntry>) {
    // Add old entries to history on start-up
    let menutree = app
        .menubar()
        .find_subtree("History")
        .expect("history menu missing");
    for entry in entries {
        let title = entry.title.clone();
        let url = entry.url.clone();
        menutree.insert_leaf(3, &title, move |app| {
            app.user_data::<Controller>()
                .expect("controller missing")
                .open_url(url.clone(), true, 0);
        });
    }
}

//--------- interface manipulation functions ---------------------------

fn move_selection(app: &mut Cursive, dir: Direction) {
    let current_view = app
        .find_name::<Layout>("main")
        .expect("main layout missing")
        .get_current_view();

    match current_view.as_str() {
        "content" => {
            let mut view = app
                .find_name::<SelectView<GopherMapEntry>>("content")
                .expect("View content missing");
            let callback = match dir {
                Direction::Next => view.select_down(1),
                Direction::Previous => view.select_up(1),
            };
            callback(app);
            if let Some(id) = view.selected_id() {
                app.find_name::<ScrollView<ResizedView<NamedView<SelectView<GopherMapEntry>>>>>(
                    "content_scroll",
                )
                .expect("gopher scroll view missing")
                .set_offset(cursive::Vec2::new(0, id));
            }
        }
        "gemini_content" => {
            let mut view = app
                .find_name::<SelectView<Option<Url>>>("gemini_content")
                .expect("View gemini_content missing");
            let callback = match dir {
                Direction::Next => view.select_down(1),
                Direction::Previous => view.select_up(1),
            };
            callback(app);
            if let Some(id) = view.selected_id() {
                app.find_name::<ScrollView<ResizedView<NamedView<SelectView<Option<Url>>>>>>(
                    "gemini_content_scroll",
                )
                .expect("gemini scroll view missing")
                .set_offset(cursive::Vec2::new(0, id));
            }
        }
        other => unreachable!("unknown view {} in main layout", other),
    }
}

fn move_to_link(app: &mut Cursive, dir: Direction) {
    let current_view = app
        .find_name::<Layout>("main")
        .expect("main layout missing")
        .get_current_view();
    match current_view.as_str() {
        "content" => move_to_link_gopher(app, dir),
        "gemini_content" => move_to_link_gemini(app, dir),
        view => unreachable!("unknown view {} in main layout", view),
    }
}

fn move_to_link_gemini(app: &mut Cursive, dir: Direction) {
    let mut view = app
        .find_name::<SelectView<Option<Url>>>("gemini_content")
        .expect("view gemini_content missing");
    let cur = view.selected_id().unwrap_or(0);
    let mut i = cur;
    match dir {
        Direction::Next => {
            i += 1; // Start at the element after the current row
            loop {
                if i >= view.len() {
                    i = 0; // Wrap and start from scratch
                    continue;
                }
                let (_, item) = view.get_item(i).unwrap();
                if i == cur {
                    break; // Once we reach the current item, we quit
                }
                if item.is_some() {
                    break;
                }
                i += 1;
            }
        }
        Direction::Previous => {
            if i > 0 {
                i -= 1; // Start at the element before the current row
            } else {
                i = view.len() - 1;
            }
            loop {
                if i == 0 {
                    i = view.len() - 1; // Wrap and start from the end
                    continue;
                }
                let (_, item) = view.get_item(i).unwrap();
                if i == cur {
                    break; // Once we reach the current item, we quit
                }
                if item.is_some() {
                    break;
                }
                i -= 1;
            }
        }
    }
    view.take_focus(cursive::direction::Direction::front()).ok();
    view.set_selection(i);

    // Scroll to selected row
    let selected_id = view.selected_id().unwrap();
    app.find_name::<ScrollView<ResizedView<NamedView<SelectView<Option<Url>>>>>>(
        "gemini_content_scroll",
    )
    .expect("gemini scroll view missing")
    .set_offset(cursive::Vec2::new(0, selected_id));
}

fn move_to_link_gopher(app: &mut Cursive, dir: Direction) {
    let mut view = app
        .find_name::<SelectView<GopherMapEntry>>("content")
        .expect("View content missing");
    let cur = view.selected_id().unwrap_or(0);
    let mut i = cur;
    match dir {
        Direction::Next => {
            i += 1; // Start at the element after the current row
            loop {
                if i >= view.len() {
                    i = 0; // Wrap and start from scratch
                    continue;
                }
                let (_, item) = view.get_item(i).unwrap();
                if i == cur {
                    break; // Once we reach the current item, we quit
                }
                if !item.item_type.is_inline() {
                    break;
                }
                i += 1;
            }
        }
        Direction::Previous => {
            if i > 0 {
                i -= 1; // Start at the element before the current row
            } else {
                i = view.len() - 1;
            }
            loop {
                if i == 0 {
                    i = view.len() - 1; // Wrap and start from the end
                    continue;
                }
                let (_, item) = view.get_item(i).unwrap();
                if i == cur {
                    break; // Once we reach the current item, we quit
                }
                if !item.item_type.is_inline() {
                    break;
                }
                i -= 1;
            }
        }
    }
    view.take_focus(cursive::direction::Direction::front()).ok();
    view.set_selection(i);

    // Scroll to selected row
    let selected_id = view.selected_id().unwrap();
    app.find_name::<ScrollView<ResizedView<NamedView<SelectView<GopherMapEntry>>>>>(
        "content_scroll",
    )
    .expect("gopher scroll view missing")
    .set_offset(cursive::Vec2::new(0, selected_id));
}

/// Moves the current selection to the next/previous item in the given vector of indices
pub(crate) fn move_to_next_item<T>(
    mut view: ViewRef<SelectView<T>>,
    mut scroll_view: ViewRef<ScrollView<ResizedView<NamedView<SelectView<T>>>>>,
    dir: Direction,
    hits: Vec<usize>,
) -> usize {
    if hits.is_empty() {
        // Not hits - don't move
        return 0;
    }
    let cur = view.selected_id().unwrap_or(0);
    let newpos = match dir {
        Direction::Next => {
            let first = hits.clone().into_iter().next().unwrap();
            match hits.into_iter().find(|&x| x > cur) {
                Some(x) => x,
                None => first, // wrap search
            }
        }
        Direction::Previous => {
            let last = hits.clone().into_iter().nth(hits.len() - 1).unwrap();
            match hits.into_iter().rev().find(|&x| x < cur) {
                Some(x) => x,
                None => last, // wrap search
            }
        }
    };
    view.take_focus(cursive::direction::Direction::front()).ok();
    view.set_selection(newpos);
    scroll_view.set_offset(cursive::Vec2::new(0, newpos));
    newpos
}
