use crate::controller::{Controller, Direction};
use crate::gophermap::{GopherMapEntry, ItemType};
use crate::ui::{dialogs, layout::Layout, statusbar::StatusBar};
use cursive::{
    event::Key,
    menu::MenuTree,
    view::{Nameable, Resizable, Scrollable},
    views::{Dialog, NamedView, OnEventView, ResizedView, ScrollView, SelectView, ViewRef},
    Cursive, View,
};
use url::Url;

const HELP: &str = include_str!("../help.txt");

pub fn setup(app: &mut Cursive) {
    trace!("ui::setup");
    setup_keys(app);
    setup_menu(app);
    setup_ui(app);
}

/// Register global keys.
fn setup_keys(app: &mut Cursive) {
    app.set_autohide_menu(false);

    // TODO: Make keys configurable
    app.add_global_callback(Key::Esc, |s| s.select_menubar());
    app.add_global_callback('q', Cursive::quit);
    app.add_global_callback('g', dialogs::open_url);
    app.add_global_callback('b', |app| {
        // step back history
        app.user_data::<Controller>()
            .expect("controller missing")
            .navigate_back();
    });
    app.add_global_callback('r', |app| {
        // reload the current page
        let index = Controller::get_selected_item_index(app);
        let controller = app.user_data::<Controller>().expect("controller missing");
        let current_url = controller.current_url.lock().unwrap().clone();
        controller.open_url(current_url, false, index);
    });
    app.add_global_callback('s', dialogs::save_as);
    app.add_global_callback('i', |app| {
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
                            let mut url = item.url.clone().into_string();
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
    app.add_global_callback('j', |app| {
        // go to next line
        move_selection(app, Direction::Next);
    });
    app.add_global_callback('k', |app| {
        // go to previous line
        move_selection(app, Direction::Previous);
    });
    app.add_global_callback('n' /*Key::Tab*/, |app| {
        // go to next link
        move_to_link(app, Direction::Next);
    });
    app.add_global_callback('p' /*Event::Shift(Key::Tab)*/, |app| {
        // go to previous link
        move_to_link(app, Direction::Previous);
    });
    app.add_global_callback('a', dialogs::add_bookmark_current_url);
    app.add_global_callback('?', |s| s.add_layer(Dialog::info(HELP)));
}

fn setup_menu(app: &mut Cursive) {
    let menubar = app.menubar();
    menubar.add_subtree(
        "File",
        MenuTree::new()
            .leaf("Open URL...", dialogs::open_url)
            .delimiter()
            .leaf("Save page as...", dialogs::save_as)
            .leaf("Settings...", dialogs::settings)
            .delimiter()
            .leaf("Quit", Cursive::quit),
    );
    menubar.add_subtree(
        "History",
        MenuTree::new()
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
        MenuTree::new()
            .leaf("Edit...", dialogs::edit_bookmarks)
            .leaf("Add bookmark", dialogs::add_bookmark_current_url)
            .delimiter(),
    );
    menubar.add_subtree(
        "Help",
        MenuTree::new()
            .subtree(
                "Help",
                MenuTree::new()
                    .leaf("Keys", |s| s.add_layer(Dialog::info(HELP)))
                    .leaf("Extended", |app| {
                        app.user_data::<Controller>()
                            .expect("controller missing")
                            .open_url(Url::parse("about:help").unwrap(), false, 0);
                    }),
            )
            .leaf("About", |s| {
                s.add_layer(Dialog::info(format!(
                    ":                       ncgopher v{:<15}            :\n\
                     :     A Gopher and Gemini client for the modern internet     :\n\
                     :              (c) 2019-2021 The ncgopher Authors            :\n\
                     :                                                            :\n\
                     :  Originally developed by Jan Schreiber <jan@mecinus.com>   :\n\
                     :                     gopher://jan.bio                       :\n\
                     :                     gemini://jan.bio                       :",
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
    view.take_focus(cursive::direction::Direction::front());
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
    view.take_focus(cursive::direction::Direction::front());
    view.set_selection(i);

    // Scroll to selected row
    let selected_id = view.selected_id().unwrap();
    app.find_name::<ScrollView<ResizedView<NamedView<SelectView<GopherMapEntry>>>>>(
        "content_scroll",
    )
    .expect("gopher scroll view missing")
    .set_offset(cursive::Vec2::new(0, selected_id));
}
