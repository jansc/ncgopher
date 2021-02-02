use crate::bookmarks::Bookmark;
use crate::history::HistoryEntry;
use crate::url_tools::download_filename_from_url;
use crate::{Controller, SETTINGS};
use cursive::{
    view::{Nameable, Resizable, Scrollable},
    views::{Checkbox, Dialog, EditView, LinearLayout, SelectView, TextView},
    Cursive,
};
use url::Url;

pub(super) fn add_bookmark_current_url(app: &mut Cursive) {
    let controller = app.user_data::<Controller>().expect("controller missing");
    let current_url = controller.current_url.lock().unwrap().clone();
    add_bookmark(app, current_url);
}

pub(crate) fn add_bookmark(app: &mut Cursive, url: Url) {
    edit_bookmark(app, url, "", "");
}

pub fn edit_bookmark(app: &mut Cursive, url: Url, title: &str, tags: &str) {
    app.add_layer(
        Dialog::new()
            .title("Add Bookmark")
            .content(
                LinearLayout::vertical()
                    .child(TextView::new("URL:"))
                    .child(
                        EditView::new()
                            .content(url.as_str())
                            .with_name("url")
                            .fixed_width(30),
                    )
                    .child(TextView::new("\nTitle:"))
                    .child(
                        EditView::new()
                            .content(title)
                            .with_name("title")
                            .fixed_width(30),
                    )
                    .child(TextView::new("Tags (comma separated):"))
                    .child(
                        EditView::new()
                            .content(tags)
                            .with_name("tags")
                            .fixed_width(30),
                    ),
            )
            .button("Ok", |app| {
                let url = app.find_name::<EditView>("url").unwrap().get_content();
                let title = app.find_name::<EditView>("title").unwrap().get_content();
                let tags = app.find_name::<EditView>("tags").unwrap().get_content();

                // Validate URL
                if let Ok(url) = Url::parse(&url) {
                    // close edit bookmark
                    app.pop_layer();
                    app.user_data::<Controller>()
                        .expect("controller missing")
                        .add_bookmark_action(url, (*title).clone(), (*tags).clone());
                } else {
                    // do not close the dialog so the user can make
                    // corrections
                    app.add_layer(Dialog::info("Invalid URL!"));
                }
            })
            .button("Cancel", |app| {
                app.pop_layer(); // Close edit bookmark
            }),
    );
}

pub(crate) fn certificate_changed(app: &mut Cursive, url: Url, fingerprint: String) {
    app.add_layer(
		Dialog::new()
			.title("Certificate warning")
			.content(TextView::new(format!("The certificate for the following domain has changed:\n{}\nDo you want to continue?", url.host_str().unwrap())))
			.button("Cancel", |app| {
				app.pop_layer(); // Close dialog
			})
			.button("Accept the risk", move |app| {
				app.pop_layer(); // Close dialog
				Controller::certificate_changed_action(app, &url, fingerprint.clone());
			})
	);
}

pub(super) fn edit_bookmarks(app: &mut Cursive) {
    let bookmarks = app
        .user_data::<Controller>()
        .expect("controller missing")
        .bookmarks
        .lock()
        .unwrap()
        .get_bookmarks();
    let mut view: SelectView<Bookmark> = SelectView::new();
    for b in bookmarks {
        let mut title = format!("{:<20}", b.title.clone().as_str());
        title.truncate(20);
        let mut url = format!("{:<50}", b.url.clone().as_str());
        url.truncate(50);
        view.add_item(format!("{} | {}", title, url), b);
    }
    app.add_layer(
        Dialog::new()
            .title("Edit bookmarks")
            .content(LinearLayout::vertical().child(view.with_name("bookmarks").scrollable()))
            .button("Delete", |app| {
                let selected = app
                    .call_on_name("bookmarks", |view: &mut SelectView<Bookmark>| {
                        view.selection()
                    })
                    .unwrap();
                match selected {
                    None => (),
                    Some(bookmark) => {
                        app.call_on_name("bookmarks", |view: &mut SelectView<Bookmark>| {
                            view.remove_item(view.selected_id().unwrap());
                        })
                        .unwrap();

                        Controller::remove_bookmark_action(app, (*bookmark).clone());
                    }
                }
            })
            .button("Open", |app| {
                let selected = app
                    .find_name::<SelectView<Bookmark>>("bookmarks")
                    .expect("bookmarks view missing")
                    .selection();
                match selected {
                    None => (),
                    Some(b) => {
                        app.user_data::<Controller>()
                            .expect("controller missing")
                            .open_url(b.url.clone(), true, 0);
                    }
                }
            })
            .button("Edit", |app| {
                let selected = app
                    .call_on_name("bookmarks", |view: &mut SelectView<Bookmark>| {
                        view.selection()
                    })
                    .unwrap();
                match selected {
                    None => (),
                    Some(b) => {
                        app.pop_layer();
                        crate::ui::dialogs::edit_bookmark(
                            app,
                            b.url.clone(),
                            &b.title,
                            &b.tags.join(","),
                        );
                    }
                }
            })
            .button("Close", |app| {
                app.pop_layer();
            }),
    );
}

pub(super) fn edit_history(app: &mut Cursive) {
    let entries = app
        .user_data::<Controller>()
        .expect("controller missing")
        .history
        .lock()
        .unwrap()
        .get_latest_history(500)
        .expect("could not get latest history");
    let mut view: SelectView<HistoryEntry> = SelectView::new();
    for e in entries {
        let mut url = e.url.to_string();
        url.truncate(50);
        view.add_item(
            format!(
                "{:>4}|{:<20}|{}",
                e.visited_count,
                e.timestamp.format("%Y-%m-%d %H:%M:%S"),
                url
            ),
            e,
        );
    }
    app.add_layer(
        Dialog::new()
            .title("Show history")
            .content(
                LinearLayout::vertical()
                    .child(TextView::new("#Vis|Last Visited        |URL"))
                    .child(LinearLayout::vertical().child(view.with_name("entries").scrollable())),
            )
            .button("Clear all history", |app| {
                app.add_layer(
                    Dialog::around(TextView::new("Do you want to delete the history?"))
                        .button("Cancel", |app| {
                            app.pop_layer();
                        })
                        .button("Yes", |app| {
                            app.pop_layer();
                            app.call_on_name("entries", |view: &mut SelectView<HistoryEntry>| {
                                view.clear()
                            });
                            app.user_data::<Controller>()
                                .expect("controller missing")
                                .clear_history();
                        }),
                );
            })
            .button("Open URL", |app| {
                let selected = app
                    .find_name::<SelectView<HistoryEntry>>("entries")
                    .unwrap()
                    .selection();
                app.pop_layer();
                match selected {
                    None => (),
                    Some(b) => {
                        app.user_data::<Controller>()
                            .expect("controller missing")
                            .open_url(b.url.clone(), true, 0);
                    }
                }
            })
            .button("Close", |app| {
                // close dialog
                app.pop_layer();
            }),
    );
}

pub(crate) fn gemini_query(app: &mut Cursive, url: Url, query: String, secret: bool) {
    app.add_layer(
        Dialog::new()
            .title(query)
            .content(
                if secret {
                    EditView::new().secret()
                } else {
                    EditView::new()
                }
                // Call `show_popup` when the user presses `Enter`
                //FIXME: create closure with url: .on_submit(search)
                .fixed_width(30)
                .with_name("query"),
            )
            .button("Cancel", |app| {
                app.pop_layer();
            })
            .button("Ok", move |app| {
                let mut url = url.clone();
                let name = app
                    .find_name::<EditView>("query")
                    .expect("query field missing")
                    .get_content();
                app.pop_layer();
                url.set_query(Some(&name));
                Controller::open_url_action(app, url.as_str());
            }),
    );
}

pub(super) fn open_url(app: &mut Cursive) {
    app.add_layer(
        Dialog::new()
            .title("Enter gopher or gemini URL:")
            .content(
                EditView::new()
                    .on_submit(|app, goto_url| {
                        app.pop_layer();
                        Controller::open_url_action(app, goto_url);
                    })
                    .with_name("goto_url")
                    .fixed_width(50),
            )
            .button("Cancel", |app| {
                app.pop_layer();
            })
            .button("Ok", |app| {
                let goto_url = app
                    .find_name::<EditView>("goto_url")
                    .expect("url field missing")
                    .get_content();
                app.pop_layer();
                Controller::open_url_action(app, &goto_url)
            }),
    );
}

pub(super) fn save_as(app: &mut Cursive) {
    let current_url = app
        .user_data::<Controller>()
        .expect("controller missing")
        .current_url
        .lock()
        .unwrap()
        .clone();

    let filename = download_filename_from_url(&current_url);

    app.add_layer(
        Dialog::new()
            .title("Enter filename:")
            .content(
                EditView::new()
                    .on_submit(Controller::save_as_action)
                    .content(filename)
                    .with_name("name")
                    .fixed_width(50),
            )
            .button("Cancel", |app| {
                app.pop_layer();
            })
            .button("Ok", |app| {
                let path = app.find_name::<EditView>("name").unwrap().get_content();
                Controller::save_as_action(app, &path);
            }),
    );
}

pub(super) fn settings(app: &mut Cursive) {
    let download_path = SETTINGS.read().unwrap().get_str("download_path").unwrap();
    let homepage_url = SETTINGS.read().unwrap().get_str("homepage").unwrap();
    let theme = SETTINGS.read().unwrap().get_str("theme").unwrap();
    let html_command = SETTINGS.read().unwrap().get_str("html_command").unwrap();
    let image_command = SETTINGS.read().unwrap().get_str("image_command").unwrap();
    let telnet_command = SETTINGS.read().unwrap().get_str("telnet_command").unwrap();
    let darkmode = theme == "darkmode";
    let textwrap = SETTINGS.read().unwrap().get_str("textwrap").unwrap();
    app.add_layer(
		Dialog::new()
			.title("Settings")
			.content(
				LinearLayout::vertical()
					.child(TextView::new("Homepage:"))
					.child(EditView::new().content(homepage_url).with_name("homepage").fixed_width(50))
					.child(TextView::new("Download path:"))
					.child(EditView::new().content(download_path.as_str()).with_name("download_path").fixed_width(50))
					.child(TextView::new("\nUse full path to the external command executable.\nIt will be called with the URL as parameter."))
					.child(TextView::new("HTML browser:"))
					.child(EditView::new().content(html_command.as_str()).with_name("html_command").fixed_width(50))
					.child(TextView::new("Images viewer:"))
					.child(EditView::new().content(image_command.as_str()).with_name("image_command").fixed_width(50))
					.child(TextView::new("Telnet client:"))
					.child(EditView::new().content(telnet_command.as_str()).with_name("telnet_command").fixed_width(50))
					.child(TextView::new("Dark mode:"))
					.child(Checkbox::new().with_checked(darkmode).with_name("darkmode"))
					.child(TextView::new("Text wrap column:"))
					.child(EditView::new().content(textwrap.as_str()).with_name("textwrap").fixed_width(5))
			)
			.button("Cancel", |app| {
				app.pop_layer();
			})
			.button("Apply",  |app| {
				let homepage = app.find_name::<EditView>("homepage").unwrap().get_content();
				let download = app.find_name::<EditView>("download_path").unwrap().get_content();
				let darkmode = app.find_name::<Checkbox>("darkmode").unwrap().is_checked();
				let html_command = app.find_name::<EditView>("html_command").unwrap().get_content();
				let image_command = app.find_name::<EditView>("image_command").unwrap().get_content();
				let telnet_command = app.find_name::<EditView>("telnet_command").unwrap().get_content();
				let textwrap = app.find_name::<EditView>("textwrap").unwrap().get_content();
				app.pop_layer();
				if Url::parse(&homepage).is_ok() {
					// only write to settings if data is correct
					SETTINGS.write().unwrap().set::<String>("homepage", homepage.to_string()).unwrap();
					SETTINGS.write().unwrap().set::<String>("download_path", download.to_string()).unwrap();
					SETTINGS.write().unwrap().set::<String>("html_command", html_command.to_string()).unwrap();
					SETTINGS.write().unwrap().set::<String>("image_command", image_command.to_string()).unwrap();
					SETTINGS.write().unwrap().set::<String>("telnet_command", telnet_command.to_string()).unwrap();
					SETTINGS.write().unwrap().set::<String>("textwrap", textwrap.to_string()).unwrap();
					let theme = if darkmode { "darkmode" } else { "lightmode" };
					app.load_toml(SETTINGS.read().unwrap().get_theme_by_name(theme.to_string())).unwrap();
					SETTINGS.write().unwrap().set::<String>("theme", theme.to_string()).unwrap();

					if let Err(why) = SETTINGS.write().unwrap().write_settings_to_file() {
						app.add_layer(Dialog::info(format!("Could not write config file: {}", why)));
					}
				} else {
					app.add_layer(Dialog::info("Invalid homepage url"));
				}
			}),
	);
}
