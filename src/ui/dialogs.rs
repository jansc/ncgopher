use crate::bookmarks::Bookmark;
use crate::clientcertificates::ClientCertificate;
use crate::history::HistoryEntry;
use crate::url_tools::download_filename_from_url;
use crate::{Controller, SETTINGS};
use cursive::{
    view::{Nameable, Resizable, Scrollable},
    views::{
        Button, Checkbox, Dialog, DummyView, EditView, LinearLayout, RadioButton, RadioGroup,
        SelectView, TextArea, TextView,
    },
    Cursive,
};
use std::time::SystemTime;
use std::vec::Vec;
use time::{format_description, Date, OffsetDateTime};
use url::{Position, Url};

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
                app.user_data::<Controller>()
                    .expect("controller missing")
                    .open_url(url.clone(), true, 0);
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

    let format = format_description::parse(
        "[year]-[month]-[day] [hour]:[minute]:[second]"
    ).expect("Could not parse timestamp format");
    for e in entries {
        let mut url = e.url.to_string();
        url.truncate(50);
        view.add_item(
            format!(
                "{:>4}|{:<20}|{}",
                e.visited_count,
                e.timestamp.format(&format).expect("Invalid timestamp from database"),
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
                .with_name("query")
                .fixed_width(30),
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
    let download_path = SETTINGS.read().unwrap().config.download_path.clone();
    let homepage_url = SETTINGS.read().unwrap().config.homepage.clone();
    let theme = SETTINGS.read().unwrap().config.theme.clone();
    let html_command = SETTINGS.read().unwrap().config.html_command.clone();
    let image_command = SETTINGS.read().unwrap().config.image_command.clone();
    let telnet_command = SETTINGS.read().unwrap().config.telnet_command.clone();
    let darkmode = theme == "darkmode";
    let textwrap = SETTINGS.read().unwrap().config.textwrap.clone();
    let disable_history = SETTINGS.read().unwrap().config.disable_history;
    let disable_identities = SETTINGS.read().unwrap().config.disable_identities;
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
                    .child(DummyView)
                    .child(LinearLayout::horizontal()
                           .child(Checkbox::new().with_checked(darkmode).with_name("darkmode"))
                           .child(DummyView)
                           .child(TextView::new("Dark mode"))
                    )
                    .child(LinearLayout::horizontal()
                           .child(Checkbox::new().with_checked(disable_history).with_name("disable_history"))
                           .child(DummyView)
                           .child(TextView::new("Disable history recording"))
                    )
                    .child(LinearLayout::horizontal()
                           .child(Checkbox::new().with_checked(disable_identities).with_name("disable_identities"))
                           .child(DummyView)
                           .child(TextView::new("Disable identities"))
                    )
                    .child(DummyView)
                    .child(LinearLayout::horizontal()
                           .child(TextView::new("Text wrap column:"))
                           .child(DummyView)
                           .child(EditView::new().content(textwrap.as_str()).with_name("textwrap").fixed_width(5))
                    )
            )
            .button("Apply",  |app| {
                let homepage = app.find_name::<EditView>("homepage").unwrap().get_content();
                let download = app.find_name::<EditView>("download_path").unwrap().get_content();
                let darkmode = app.find_name::<Checkbox>("darkmode").unwrap().is_checked();
                let disable_history = app.find_name::<Checkbox>("disable_history").unwrap().is_checked();
                let disable_identities = app.find_name::<Checkbox>("disable_identities").unwrap().is_checked();
                let html_command = app.find_name::<EditView>("html_command").unwrap().get_content();
                let image_command = app.find_name::<EditView>("image_command").unwrap().get_content();
                let telnet_command = app.find_name::<EditView>("telnet_command").unwrap().get_content();
                let textwrap = app.find_name::<EditView>("textwrap").unwrap().get_content();
                app.pop_layer();
                if Url::parse(&homepage).is_ok() {
                    // only write to settings if data is correct
                    SETTINGS.write().unwrap().config.homepage = homepage.to_string();
                    SETTINGS.write().unwrap().config.download_path = download.to_string();
                    SETTINGS.write().unwrap().config.html_command = html_command.to_string();
                    SETTINGS.write().unwrap().config.image_command = image_command.to_string();
                    SETTINGS.write().unwrap().config.telnet_command = telnet_command.to_string();
                    SETTINGS.write().unwrap().config.textwrap = textwrap.to_string();
                    SETTINGS.write().unwrap().config.disable_history = disable_history;
                    SETTINGS.write().unwrap().config.disable_identities = disable_identities;
                    let theme = if darkmode { "darkmode" } else { "lightmode" };
                    app.load_toml(SETTINGS.read().unwrap().get_theme_by_name(theme.to_string())).unwrap();
                    SETTINGS.write().unwrap().config.theme = theme.to_string();

                    if let Err(why) = SETTINGS.write().unwrap().write_settings_to_file() {
                        app.add_layer(Dialog::info(format!("Could not write config file: {}", why)));
                    }
                } else {
                    app.add_layer(Dialog::info("Invalid homepage url"));
                }
            })
            .button("Cancel", |app| {
                app.pop_layer();
            })
    );
}

pub(crate) fn manage_client_certificates(app: &mut Cursive) {
    let client_certificates = app
        .user_data::<Controller>()
        .expect("controller missing")
        .client_certificates
        .lock()
        .unwrap()
        .get_client_certificates();
    let mut view: SelectView<ClientCertificate> = SelectView::new();
    for cc in client_certificates {
        let mut common_name = format!("{:<30}", cc.common_name.clone().as_str());
        let format =
            format_description::parse("[year]-[month]-[day]").expect("Could not parse date format");
        let now = OffsetDateTime::now_utc().date();
        let expiration_date = format!("{:<10}", cc.expiration_date.format(&format).unwrap());
        let warning = if now > cc.expiration_date { "!" } else { " " };

        let urls = app
            .user_data::<Controller>()
            .expect("controller missing")
            .client_certificates
            .lock()
            .unwrap()
            .get_urls_for_certificate(&cc.fingerprint);
        let used_on = match urls.len() {
            0 => "Unused".to_string(),
            1 => "1 URL".to_string(),
            _ => format!("{} URLs", urls.len()),
        };

        common_name.truncate(30);
        view.add_item(
            format!(
                "{} | {}{} | {}",
                common_name, warning, expiration_date, used_on
            ),
            cc,
        );
    }
    app.add_layer(
        Dialog::new()
            .title("Edit identities")
            .content(
                LinearLayout::vertical().child(view.with_name("client_certificates").scrollable()),
            )
            .button("Create identity", |app| {
                app.pop_layer();
                add_client_certificate(app, None);
            })
            .button("Delete", |app| {
                let selected = app
                    .call_on_name(
                        "client_certificates",
                        |view: &mut SelectView<ClientCertificate>| view.selection(),
                    )
                    .unwrap();
                app.add_layer(
                    Dialog::around(TextView::new("Do you really want to delete this identity?"))
                        .button("Delete", move |app| {
                            app.pop_layer(); // Confirm dialog
                            match &selected {
                                None => (),
                                Some(client_certificate) => {
                                    app.call_on_name(
                                        "client_certificates",
                                        |view: &mut SelectView<ClientCertificate>| {
                                            view.remove_item(view.selected_id().unwrap());
                                        },
                                    )
                                    .unwrap();
                                    Controller::remove_client_certificate_action(
                                        app,
                                        client_certificate,
                                    );
                                }
                            }
                        })
                        .dismiss_button("Cancel"),
                );
            })
            .button("Edit", |app| {
                let selected = app
                    .call_on_name(
                        "client_certificates",
                        |view: &mut SelectView<ClientCertificate>| view.selection(),
                    )
                    .unwrap();
                if let Some(cc) = selected {
                    app.pop_layer();
                    crate::ui::dialogs::edit_client_certificate(app, (*cc).clone());
                };
            })
            .button("Close", |app| {
                app.pop_layer();
            }),
    );
}

pub(crate) fn choose_client_certificate(app: &mut Cursive, url: Url) {
    let client_certificates = app
        .user_data::<Controller>()
        .expect("controller missing")
        .client_certificates
        .lock()
        .unwrap()
        .get_client_certificates();
    let mut view: SelectView<ClientCertificate> = SelectView::new();
    for cc in client_certificates {
        let mut common_name = format!("{:<30}", cc.common_name.clone().as_str());
        let format =
            format_description::parse("[year]-[month]-[day]").expect("Could not parse date format");
        let now = OffsetDateTime::now_utc().date();
        let expiration_date = format!("{:<10}", cc.expiration_date.format(&format).unwrap());
        let warning = if now > cc.expiration_date { "!" } else { " " };

        let urls = app
            .user_data::<Controller>()
            .expect("controller missing")
            .client_certificates
            .lock()
            .unwrap()
            .get_urls_for_certificate(&cc.fingerprint);
        let used_on = match urls.len() {
            0 => "Unused".to_string(),
            1 => "1 URL".to_string(),
            _ => format!("{} URLs", urls.len()),
        };

        common_name.truncate(30);
        view.add_item(
            format!(
                "{} | {}{} | {}",
                common_name, warning, expiration_date, used_on
            ),
            cc,
        );
    }
    let original_url = url.clone();
    app.add_layer(
        Dialog::new()
            .title("Choose identity")
            .content(
                LinearLayout::vertical()
                    .child(TextView::new(
                        "The current gemini site requests a client certificate.\n\
                                          Select an identity or create a new one to continue.",
                    ))
                    .child(DummyView)
                    .child(view.with_name("client_certificates").scrollable()),
            )
            .button("Create identity", move |app| {
                app.pop_layer();
                add_client_certificate(app, Some(original_url.clone()));
            })
            .button("Use identity", move |app| {
                let selected = app
                    .call_on_name(
                        "client_certificates",
                        |view: &mut SelectView<ClientCertificate>| view.selection(),
                    )
                    .unwrap();
                if let Some(cc) = selected {
                    app.pop_layer();
                    let controller = app.user_data::<Controller>().expect("controller missing");
                    let mut guard = controller.client_certificates.lock().unwrap();
                    guard.use_current_site(&url, &cc.fingerprint);
                    drop(guard);
                    controller.fetch_gemini_url(url.clone(), 0);
                };
            })
            .button("Cancel", |app| {
                app.pop_layer();
            }),
    );
}

pub enum UrlOriginType {
    DecideLater,
    CurrentHost,
    CurrentUrl,
    SpecifiedUrl,
}

pub fn add_client_certificate(app: &mut Cursive, url: Option<Url>) {
    /*
    - New certificate -
    | Common name:              |
    | _________________         |
    |---------------------------|
    | Use on:                   |
    | [Not used]                |
    | [Current host]            |
    | [Current URL]             |
    | [This URL:]               |
    | Enter URL:                |
    | _________________         |
    |---------------------------|
    | Valid until (YYYY-MM-DD): |
    | _________________         |
    |---------------------------|
    | Notes:                    |
    | _________________         |
    | _________________         |
    | <Cancel> <Save>           |
    |                           |
     */

    let mut valid_for_group: RadioGroup<UrlOriginType> = RadioGroup::new();
    valid_for_group.set_on_change(|app, selected| {
        let mut specified_url = app.find_name::<EditView>("specified_url").unwrap();
        let controller = app.user_data::<Controller>().expect("controller missing");
        let current_url = controller.current_url.lock().unwrap().clone();

        // For current urls: drop URL parameters
        let u = Url::parse(&current_url[..Position::AfterPath]).unwrap();
        let mut current_host = Url::parse("gemini://example.com").expect("Unable to parse url");
        current_host.set_host(current_url.host_str()).ok();
        current_host.set_port(current_url.port()).ok();
        current_host.set_scheme("gemini").ok();
        match selected {
            UrlOriginType::DecideLater => specified_url.set_content(""),
            UrlOriginType::CurrentUrl => specified_url.set_content(u),
            UrlOriginType::CurrentHost => specified_url.set_content(current_host),
            UrlOriginType::SpecifiedUrl => specified_url.set_content("gemini://host"),
        };
    });

    // Calculate default expiry date:
    let odt: OffsetDateTime = SystemTime::now().into();
    let mut date: Date = odt.date();
    date = date
        .replace_year(date.year() + 1)
        .expect("Cannot create expiry date");
    let format =
        format_description::parse("[year]-[month]-[day]").expect("Could not parse date format");
    let expiry_date = date.format(&format).unwrap();
    let original_url = url.clone();

    app.add_layer(
        Dialog::new()
            .title("New identity")
            .content(
                LinearLayout::vertical()
                    .child(TextView::new("Name:"))
                    .child(
                        EditView::new()
                            .with_name("common_name")
                            .fixed_width(40),
                    )
                    .child(DummyView)
                    .child(TextView::new("Use on:"))
                    .child(
                        LinearLayout::vertical()
                            .child(valid_for_group.button(UrlOriginType::DecideLater, "Decide later"))
                            .child(valid_for_group.button(UrlOriginType::CurrentHost, "Current host"))
                            .child(valid_for_group.button(UrlOriginType::CurrentUrl, "Current URL").with_name("current_url_button"))
                            .child(valid_for_group.button(UrlOriginType::SpecifiedUrl, "Specified URL:").with_name("specified_url_button"))
                            .child(EditView::new()
                                   .on_edit(move |app, _text, _cursor| {
                                       app.find_name::<RadioButton<UrlOriginType>>("specified_url_button").unwrap().select();
                                   })
                                   .with_name("specified_url")
                                   .fixed_width(40)
                                )
                    )
                    .child(DummyView)
                    .child(TextView::new("Valid until (YYYY-MM-DD):"))
                    .child(
                        EditView::new()
                            .content(expiry_date.as_str())
                            .with_name("valid_until")
                            .fixed_width(40),
                    )
                    .child(DummyView)
                    .child(TextView::new("Notes:"))
                    .child(TextArea::new()
                           .with_name("notes")
                           .fixed_width(40)
                           .min_height(2)
                           )
                    )
            .button("Ok", move |app| {
                let common_name = app.find_name::<EditView>("common_name").unwrap().get_content();
                let notes = app.find_name::<TextArea>("notes").unwrap().get_content().to_string();
                let valid_until = app.find_name::<EditView>("valid_until").unwrap().get_content();
                let specified_url = app.find_name::<EditView>("specified_url").unwrap().get_content();

                let mut parse_error: bool = false;

                // Check if common_name is not empty (Maybe: if common_name is unique)
                if common_name.is_empty() {
                    app.add_layer(Dialog::info("You have to provide a name. The name cannon be changed after\nthe identity has been created."));
                    return;
                }

                // Validate date
                let format = format_description::parse(
                    "[year]-[month]-[day]"
                ).expect("Could not parse date format");
                let valid_until_date : Date = date;
                if let Ok(valid_until_date) = Date::parse(&valid_until, &format) {
                    info!("Parsed client certificate date: {:?}", valid_until_date);
                } else {
                    info!("Could not parse date {}", valid_until);
                    app.add_layer(Dialog::info("Invalid date format. Must be YYYY-MM-DD."));
                    return;
                }

                // validate url if not empty
                let parsed_url : Option<Url>= if specified_url.is_empty() { None } else {
                    match Url::parse(&specified_url) {
                        Ok(url) => {
                            if let Ok(u) = Url::parse(&url[..Position::AfterPath]) {
                                Some(u)
                            } else {
                                parse_error = true;
                                None
                            }
                        },
                        Err(_err) => {
                            parse_error = true;
                            None
                        }
                    }
                };
                if parse_error {
                    app.add_layer(Dialog::info("Provided URL is invalid."));
                    return;
                }
                if let Some(ref pu) = parsed_url {
                    if pu.scheme() != "gemini" {
                        app.add_layer(Dialog::info("The specified URL is not a gemini URL."));
                        return;
                    }
                }

                let controller = app.user_data::<Controller>().expect("controller missing");
                controller.create_client_certificate(common_name.to_string(), notes, valid_until_date, parsed_url);
                app.pop_layer();
                if let Some(original_url) = &original_url {
                    let controller = app.user_data::<Controller>().expect("controller missing");
                    controller.fetch_gemini_url(original_url.clone(), 0);
                }
            })
            .button("Cancel", |app| {
                app.pop_layer(); // Close dialog
            }),
    );

    // If a URL is given, we're about to open this URL from fetch_gemini_url()
    // So we set the URL as the specified URL (since the default would be
    // "decide later")
    if let Some(url) = &url {
        // Strip URL parameters:
        let u = Url::parse(&url[..Position::AfterPath]).unwrap();
        let mut specified_url = app.find_name::<EditView>("specified_url").unwrap();
        specified_url.set_content(u);
        app.find_name::<RadioButton<UrlOriginType>>("current_url_button")
            .unwrap()
            .select();
    }
}

/*
| Common name:                                           |
| _________________ (read only)                          |
|--------------------------------------------------------|
| Use on URLs:                                           |
| [gemini://jan.bio/glog/]                               |
| [gemini://astrobotany.mozz.us]                         |
| <Add uri> <Remove uri>                                 |
|--------------------------------------------------------|
| Notes:                                                 |
| _________________                                      |
| _________________                                      |
| <Delete Identity> <Use on current site> <Save> <Close> |
*/
pub fn edit_client_certificate(app: &mut Cursive, cc: ClientCertificate) {
    let client_certificate_to_delete = cc.clone();
    let client_certificate_to_url = cc.clone();
    let client_certificate = cc.clone();
    let note = cc.note.clone();
    let mut view: SelectView<Url> = SelectView::new();

    let urls = app
        .user_data::<Controller>()
        .expect("controller missing")
        .client_certificates
        .lock()
        .unwrap()
        .get_urls_for_certificate(&cc.fingerprint);
    for url_str in urls.iter() {
        view.add_item(url_str, Url::parse(url_str).unwrap());
    }
    app.add_layer(
        Dialog::new()
            .title("Edit identity")
            .content(
                LinearLayout::vertical()
                    .child(TextView::new("Common name:"))
                    .child(
                        EditView::new()
                            .content(cc.common_name.as_str())
                            .with_enabled(false)
                            .with_name("common_name"),
                    )
                    .child(DummyView)
                    .child(TextView::new("Use on URLs:"))
                    .child(view.with_name("urls"))
                    .child(
                        LinearLayout::horizontal()
                            .child(Button::new("Add URL", |app| {
                                add_url_to_client_certificate(app);
                            }))
                            .child(DummyView)
                            .child(Button::new("Remove URL", |app| {
                                let selected = app
                                    .call_on_name("urls", |view: &mut SelectView<Url>| {
                                        view.selection()
                                    })
                                    .unwrap();
                                match selected {
                                    None => (),
                                    Some(_) => {
                                        app.call_on_name("urls", |view: &mut SelectView<Url>| {
                                            view.remove_item(view.selected_id().unwrap());
                                        })
                                        .unwrap();
                                    }
                                }
                            })),
                    )
                    .child(DummyView)
                    .child(TextView::new("Notes:"))
                    .child(
                        TextArea::new()
                            .content(note)
                            .with_name("notes")
                            .min_height(2),
                    ),
            )
            .button("Delete identity", move |app| {
                let cc = client_certificate_to_delete.clone();
                app.add_layer(
                    Dialog::around(TextView::new("Do you really want to delete this identity?"))
                        .button("Delete", move |app| {
                            Controller::remove_client_certificate_action(app, &cc);
                            app.pop_layer(); // Confirm dialog
                            app.pop_layer(); // Edit client certificate dialog
                            manage_client_certificates(app);
                        })
                        .dismiss_button("Cancel"),
                );
            })
            .button("Use on current site", move |app| {
                if Controller::use_current_site_client_certificate_action(
                    app,
                    client_certificate_to_url.clone(),
                ) {
                    app.pop_layer();
                    manage_client_certificates(app);
                } else {
                    app.add_layer(Dialog::info("The current URL is not a gemini URL."));
                }
            })
            .button("Save", move |app| {
                let note = app
                    .find_name::<TextArea>("notes")
                    .expect("Could not find notes")
                    .get_content()
                    .to_string();
                let mut urls: Vec<Url> = Vec::<Url>::new();
                for (_label, item) in app
                    .find_name::<SelectView<Url>>("urls")
                    .expect("Could not find urls")
                    .iter()
                {
                    urls.push(item.clone());
                }
                let controller = app.user_data::<Controller>().expect("controller missing");
                let mut client_certificate = client_certificate.clone();
                client_certificate.note = note;
                controller.update_client_certificate(&client_certificate, urls);

                app.pop_layer();
                manage_client_certificates(app);
            })
            .button("Cancel", |app| {
                app.pop_layer();
                manage_client_certificates(app);
            }),
    );
}

/// Dialog that adds a URL to a client certificate (called from edit_client_Certificate).
/// Should maybe generalized.
pub fn add_url_to_client_certificate(app: &mut Cursive) {
    app.add_layer(
        Dialog::new()
            .title("Edit client certificate")
            .content(
                LinearLayout::vertical().child(TextView::new("URL:")).child(
                    EditView::new()
                        .content("gemini://")
                        .with_name("url")
                        .fixed_width(30),
                ),
            )
            .button("Add", move |app| {
                let url = app.find_name::<EditView>("url").unwrap().get_content();
                if let Ok(parsed_url) = Url::parse(&url.to_string()) {
                    app.pop_layer();
                    app.call_on_name("urls", |view: &mut SelectView<Url>| {
                        view.add_item(url.to_string(), parsed_url);
                    })
                    .unwrap();
                } else {
                    app.add_layer(Dialog::info("Invalid URL"));
                }
            })
            .button("Cancel", move |app| {
                app.pop_layer();
            }),
    );
}
