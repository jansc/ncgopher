use crate::bookmarks::Bookmark;
use crate::controller::ControllerMessage;
use crate::gemini::{GeminiLine, GeminiType, LineType};
use crate::gophermap::{GopherMapEntry, ItemType};
use crate::history::HistoryEntry;
use cursive::event::Key;
use cursive::menu::MenuTree;
use cursive::traits::*;
use cursive::utils::markup::StyledString;
use cursive::views::{
    Checkbox, Dialog, EditView, LinearLayout, NamedView, OnEventView, ResizedView, ScrollView,
    SelectView, TextView, ViewRef,
};
use cursive::Cursive;
use regex::Regex;
use std::str;
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use textwrap::wrap_iter;
use url::Url;
//use crate::settings::Settings;
use crate::ui;
use crate::ui::layout::Layout;
use crate::ui::statusbar::StatusBar;
use crate::SETTINGS;

extern crate chrono;
extern crate log;
extern crate url;

#[derive(Clone, Debug)]
pub enum Direction {
    Next,
    Previous,
}

/// Messages sent between Controller and UI
pub enum UiMessage {
    AddToBookmarkMenu(Bookmark),
    AddToHistoryMenu(HistoryEntry),
    BinaryWritten(String, usize),
    ClearHistoryMenu,
    MoveSelection(Direction),
    MoveToLink(Direction),
    ClearBookmarksMenu,
    OpenQueryDialog(Url),
    OpenGeminiQueryDialog(Url, String),
    OpenQueryUrl(Url),
    // TODO: Remove this
    OpenUrl(Url, bool, usize),
    OpenUrlFromString(String, bool, usize),
    PageSaved(Url, String),
    Quit,
    ShowAddBookmarkDialog(Bookmark),
    ShowEditHistoryDialog(Vec<HistoryEntry>),
    ShowContent(Url, String, ItemType, usize),
    ShowCertificateChangedDialog(Url, String),
    ShowGeminiContent(Url, GeminiType, String),
    ShowEditBookmarksDialog(Vec<Bookmark>),
    ShowLinkInfo,
    ShowMessage(String),
    ShowURLDialog,
    ShowSaveAsDialog(Url),
    ShowSearchDialog(Url),
    ShowSettingsDialog,
    Trigger,
}

/// UserData is stored inside the cursive object (with set_user_data).
/// This makes the contained data available without the use of closures.
#[derive(Clone)]
pub struct UserData {
    pub ui_tx: Arc<RwLock<mpsc::Sender<UiMessage>>>,
    pub controller_tx: Arc<RwLock<mpsc::Sender<ControllerMessage>>>,
}

impl UserData {
    pub fn new(
        ui_tx: Arc<RwLock<mpsc::Sender<UiMessage>>>,
        controller_tx: Arc<RwLock<mpsc::Sender<ControllerMessage>>>,
    ) -> UserData {
        UserData {
            ui_tx,
            controller_tx,
        }
    }
}

/// Struct representing the visible part of NcGopher (=the UI).
#[derive(Clone)]
pub struct NcGopher {
    app: Arc<RwLock<Cursive>>,
    ui_rx: Arc<mpsc::Receiver<UiMessage>>,
    pub ui_tx: Arc<RwLock<mpsc::Sender<UiMessage>>>,
    pub controller_tx: Arc<RwLock<mpsc::Sender<ControllerMessage>>>,
    /// Message shown in statusbar
    message: Arc<RwLock<String>>,
    is_running: bool,
}

impl Drop for NcGopher {
    fn drop(&mut self) {
        // Cleanup
    }
}

impl NcGopher {
    pub fn new(cursive: Cursive, controller_tx: mpsc::Sender<ControllerMessage>) -> NcGopher {
        let (ui_tx, ui_rx) = mpsc::channel::<UiMessage>();
        let ncgopher = NcGopher {
            app: Arc::new(RwLock::new(cursive)),
            ui_tx: Arc::new(RwLock::new(ui_tx)),
            ui_rx: Arc::new(ui_rx),
            controller_tx: Arc::new(RwLock::new(controller_tx)),
            message: Arc::new(RwLock::new(String::new())),
            is_running: true,
        };
        // Make channels available from callbacks
        let userdata = UserData::new(ncgopher.ui_tx.clone(), ncgopher.controller_tx.clone());
        ncgopher.app.write().unwrap().set_user_data(userdata);

        ncgopher
    }

    /// Used by statusbar to get current message
    pub fn get_message(&self) -> String {
        self.message.read().unwrap().clone()
    }

    /// Sets message for statusbar
    fn set_message(&mut self, msg: &str) {
        let mut message = self.message.write().unwrap();
        message.clear();
        message.push_str(msg);
        self.trigger();
    }

    /// Setup of UI, register global keys
    pub fn setup_ui(&mut self) {
        cursive::logger::init();
        info!("NcGopher::setup_ui()");
        self.create_menubar();
        let mut app = self.app.write().unwrap();

        app.set_autohide_menu(false);

        // TODO: Make keys configurable
        app.add_global_callback('q', |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata.ui_tx.read().unwrap().send(UiMessage::Quit)
            });
        });
        app.add_global_callback('g', |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .ui_tx
                    .read()
                    .unwrap()
                    .clone()
                    .send(UiMessage::ShowURLDialog)
                    .unwrap()
            });
        });
        app.add_global_callback('b', |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .controller_tx
                    .read()
                    .unwrap()
                    .send(ControllerMessage::NavigateBack)
            });
        });
        app.add_global_callback('r', |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .controller_tx
                    .read()
                    .unwrap()
                    .send(ControllerMessage::ReloadCurrentPage)
            });
        });
        app.add_global_callback('s', |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .controller_tx
                    .read()
                    .unwrap()
                    .clone()
                    .send(ControllerMessage::RequestSaveAsDialog)
                    .unwrap()
            });
        });
        app.add_global_callback('i', |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .ui_tx
                    .read()
                    .unwrap()
                    .clone()
                    .send(UiMessage::ShowLinkInfo)
                    .unwrap()
            });
        });
        app.add_global_callback('j', |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .ui_tx
                    .read()
                    .unwrap()
                    .clone()
                    .send(UiMessage::MoveSelection(Direction::Next))
                    .unwrap()
            });
        });
        app.add_global_callback('k', |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .ui_tx
                    .read()
                    .unwrap()
                    .clone()
                    .send(UiMessage::MoveSelection(Direction::Previous))
                    .unwrap()
            });
        });
        app.add_global_callback('p' /*Event::Shift(Key::Tab)*/, |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .ui_tx
                    .read()
                    .unwrap()
                    .clone()
                    .send(UiMessage::MoveToLink(Direction::Previous))
                    .unwrap()
            });
        });
        app.add_global_callback('n' /*Key::Tab*/, |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .ui_tx
                    .read()
                    .unwrap()
                    .clone()
                    .send(UiMessage::MoveToLink(Direction::Next))
                    .unwrap()
            });
        });
        app.add_global_callback('a', |app| {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .controller_tx
                    .read()
                    .unwrap()
                    .clone()
                    .send(ControllerMessage::RequestAddBookmarkDialog)
                    .unwrap()
            });
        });
        app.add_global_callback(Key::Esc, |s| s.select_menubar());
        app.add_global_callback('?', |s| s.add_layer(Dialog::info(include_str!("help.txt"))));

        // Create text view
        let textview = SelectView::<String>::new();
        let scrollable_textview = textview
            .with_name("text")
            .full_width()
            .scrollable()
            .with_name("text_scroll");
        let text_event_view = OnEventView::new(scrollable_textview).on_event(' ', |app| {
            app.call_on_name(
                "text_scroll",
                |s: &mut ScrollView<ResizedView<NamedView<SelectView>>>| {
                    let rect = s.content_viewport();
                    let bl = rect.bottom_left();
                    s.set_offset(bl);
                },
            );
        });

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
        let view: SelectView<GeminiLine> = SelectView::new();
        let scrollable = view
            .with_name("gemini_content")
            .full_width()
            .scrollable()
            .with_name("gemini_content_scroll");
        let gemini_event_view = OnEventView::new(scrollable).on_event(' ', |app| {
            app.call_on_name(
                "gemini_content_scroll",
                |s: &mut ScrollView<ResizedView<NamedView<SelectView<GeminiLine>>>>| {
                    let rect = s.content_viewport();
                    let bl = rect.bottom_left();
                    s.set_offset(bl);
                },
            );
        });
        let status = StatusBar::new(Arc::new(self.clone())).with_name("statusbar");
        let mut layout = Layout::new(status /*, theme*/)
            .view("text", text_event_view, "Textfile")
            .view("content", event_view, "Gophermap")
            .view("gemini_content", gemini_event_view, "Gemini");
        layout.set_view("content");
        app.add_fullscreen_layer(layout.with_name("main"));

        app.add_global_callback('~', Cursive::toggle_debug_console);
    }

    // TODO: Should be moved to controller
    fn get_filename_from_url(&mut self, url: Url) -> String {
        let mut segments = url.path_segments().map(|c| c.collect::<Vec<_>>()).unwrap();
        let last_seg = segments.pop();
        if let Some(filename) = last_seg {
            return filename.to_string();
        }
        "download.bin".to_string()
    }

    fn binary_written(&mut self, filename: String, bytes: usize) {
        self.set_message(format!("File downloaded: {} ({} bytes)", filename, bytes).as_str());
    }

    pub fn create_menubar(&mut self) {
        let mut app = self.app.write().unwrap();
        let menubar = app.menubar();
        menubar.add_subtree(
            "File",
            MenuTree::new()
                .leaf("Open URL...", |app| {
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .clone()
                            .send(UiMessage::ShowURLDialog)
                            .unwrap()
                    });
                })
                .delimiter()
                .leaf("Save page as...", |app| {
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .controller_tx
                            .read()
                            .unwrap()
                            .clone()
                            .send(ControllerMessage::RequestSaveAsDialog)
                            .unwrap()
                    });
                })
                .leaf("Settings...", |app| {
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .controller_tx
                            .read()
                            .unwrap()
                            .clone()
                            .send(ControllerMessage::RequestSettingsDialog)
                            .unwrap()
                    });
                })
                .delimiter()
                .leaf("Quit", |app| {
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .clone()
                            .send(UiMessage::Quit)
                            .unwrap()
                    });
                }),
        );
        menubar.add_subtree(
            "History",
            MenuTree::new()
                .leaf("Show all history...", |s| {
                    s.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .controller_tx
                            .read()
                            .unwrap()
                            .clone()
                            .send(ControllerMessage::RequestEditHistoryDialog)
                            .unwrap()
                    });
                })
                .leaf("Clear history", |app| {
                    app.add_layer(
                        Dialog::around(TextView::new("Do you want to delete the history?"))
                            .button("Cancel", |app| {
                                app.pop_layer();
                            })
                            .button("Ok", |app| {
                                app.pop_layer();
                                app.with_user_data(|userdata: &mut UserData| {
                                    userdata
                                        .controller_tx
                                        .read()
                                        .unwrap()
                                        .send(ControllerMessage::ClearHistory)
                                        .unwrap()
                                });
                            }),
                    );
                })
                .delimiter(),
        );
        menubar.add_subtree(
            "Bookmarks",
            MenuTree::new()
                .leaf("Edit...", |app| {
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .controller_tx
                            .read()
                            .unwrap()
                            .send(ControllerMessage::RequestEditBookmarksDialog)
                            .unwrap()
                    });
                })
                .leaf("Add bookmark", |app| {
                    //app.add_layer(Dialog::info("Add bookmark not implemented"))
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .controller_tx
                            .read()
                            .unwrap()
                            .clone()
                            .send(ControllerMessage::RequestAddBookmarkDialog)
                            .unwrap()
                    });
                })
                .delimiter(),
        );
        menubar.add_subtree(
            "Search",
            MenuTree::new()
                .leaf("Veronica/2...", |app| {
                    let url = Url::parse("gopher://gopher.floodgap.com:70/7/v2/vs").unwrap();
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowSearchDialog(url))
                            .unwrap()
                    });
                })
                .leaf("Gopherpedia...", |app| {
                    let url = Url::parse("gopher://gopherpedia.com:70/7/lookup").unwrap();
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowSearchDialog(url))
                            .unwrap()
                    });
                })
                .leaf("Gopher Movie Database...", |app| {
                    let url = Url::parse("gopher://jan.bio:70/7/cgi-bin/gmdb.py").unwrap();
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowSearchDialog(url))
                            .unwrap()
                    });
                })
                .leaf("OpenBSD man pages...", |app| {
                    let url = Url::parse("gopher://perso.pw:70/7/man.dcgi").unwrap();
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowSearchDialog(url))
                            .unwrap()
                    });
                })
                .leaf("searx search...", |app| {
                    let url = Url::parse("gopher://me0w.net:70/7/searx.dcgi").unwrap();
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowSearchDialog(url))
                            .unwrap()
                    });
                })
                .leaf("[gemini] GUS...", |app| {
                    let url = Url::parse("gemini://gus.guru/search").unwrap();
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::OpenGeminiQueryDialog(
                                url,
                                "Enter query".to_string(),
                            ))
                            .unwrap()
                    });
                })
                .leaf("[gemini] Houston...", |app| {
                    let url = Url::parse("gemini://houston.coder.town/search").unwrap();
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::OpenGeminiQueryDialog(
                                url,
                                "Enter query".to_string(),
                            ))
                            .unwrap()
                    });
                }),
        );
        menubar.add_subtree(
            "Help",
            MenuTree::new()
                .subtree(
                    "Help",
                    MenuTree::new()
                        .leaf("General", |s| {
                            s.add_layer(Dialog::info(include_str!("help.txt")))
                        })
                        .leaf("Online", |app| {
                            app.with_user_data(|userdata: &mut UserData| {
                                userdata
                                    .ui_tx
                                    .write()
                                    .unwrap()
                                    .send(UiMessage::OpenUrlFromString(
                                        "gopher://jan.bio/1/ncgopher/".to_string(),
                                        false,
                                        0,
                                    ))
                                    .unwrap();
                            });
                        }),
                )
                .leaf("About", |s| {
                    s.add_layer(Dialog::info(format!(
                        ";               ncgopher v{}\n\
                         ;     A Gopher client for the modern internet\n\
                         ; (c) 2019-2020 by Jan Schreiber <jan@mecinus.com>\n\
                         ;               gopher://jan.bio",
                        env!("CARGO_PKG_VERSION")
                    )))
                }),
        );
    }

    pub fn open_url_string(&mut self, url: String, add_to_history: bool, index: usize) {
        let mut url = url;

        // Default-protocol is gopher
        let scheme_regex = Regex::new(r"^[a-zA-Z]+://").unwrap();
        if let Some(_scheme) = scheme_regex.captures(&url) {
            // Scheme present
        } else {
            url.insert_str(0, "gopher://");
        }

        let res = Url::parse(url.as_str());
        match res {
            Ok(url) => {
                self.open_url(url, add_to_history, index);
            }
            Err(e) => {
                self.set_message(format!("Invalid URL: {}", e).as_str());
            }
        }
    }

    pub fn open_url(&mut self, url: Url, add_to_history: bool, index: usize) {
        match url.scheme() {
            "gopher" => self.open_gopher_address(
                url.clone(),
                ItemType::from_url(&url),
                add_to_history,
                index,
            ),
            "gemini" => self.open_gemini_address(url.clone(), add_to_history, index),
            "http" | "https" => {
                self.controller_tx
                    .read()
                    .unwrap()
                    .send(ControllerMessage::OpenHtml(url.clone()))
                    .unwrap();
            }
            // .send(ControllerMessage::OpenHtml(entry.url.clone()))
            _ => self.set_message(format!("Invalid URL: {}", url).as_str()),
        }
    }

    pub fn open_gemini_address(&mut self, url: Url, add_to_history: bool, index: usize) {
        self.set_message("Loading ...");
        let mut app = self.app.write().unwrap();
        app.call_on_name("main", |v: &mut ui::layout::Layout| {
            v.set_view("gemini_content");
        });
        self.controller_tx
            .read()
            .unwrap()
            .send(ControllerMessage::FetchGeminiUrl(
                url,
                add_to_history,
                index,
            ))
            .unwrap();
    }

    pub fn open_gopher_address(
        &mut self,
        url: Url,
        item_type: ItemType,
        add_to_history: bool,
        index: usize,
    ) {
        self.set_message("Loading ...");
        {
            let mut app = self.app.write().unwrap();
            app.call_on_name("main", |v: &mut ui::layout::Layout| {
                v.set_view("content");
            });
        }

        if ItemType::is_download(item_type) {
            match dirs::home_dir() {
                Some(dir) => {
                    let filename = self.get_filename_from_url(url.clone());
                    let local_path = dir.into_os_string().into_string().unwrap();
                    let path = format!("{}/{}", local_path, filename);
                    self.controller_tx
                        .read()
                        .unwrap()
                        .send(ControllerMessage::FetchBinaryUrl(url, path))
                        .unwrap();
                }
                None => {
                    self.set_message("Could not find download dir");
                }
            };
        } else {
            self.controller_tx
                .read()
                .unwrap()
                .send(ControllerMessage::FetchUrl(
                    url,
                    item_type,
                    add_to_history,
                    index,
                ))
                .unwrap();
        }
    }

    fn open_query_dialog(&mut self, url: Url) {
        {
            let mut app = self.app.write().unwrap();
            app.add_layer(
                Dialog::new()
                    .title("Enter query:")
                    .content(
                        EditView::new()
                            // Call `show_popup` when the user presses `Enter`
                            //FIXME: create closure with url: .on_submit(search)
                            .with_name("query")
                            .fixed_width(30),
                    )
                    .button("Cancel", move |app| {
                        app.pop_layer();
                    })
                    .button("Ok", move |app| {
                        let name =
                            app.call_on_name("query", |view: &mut EditView| view.get_content());
                        if let Some(n) = name {
                            let mut u = url.clone();
                            let mut path = u.path().to_string();
                            path.push_str("%09");
                            path.push_str(&n);
                            u.set_path(path.as_str());
                            info!("open_query_dialog(): url = {}", u);

                            app.pop_layer(); // Close search dialog
                            app.with_user_data(|userdata: &mut UserData| {
                                userdata
                                    .ui_tx
                                    .write()
                                    .unwrap()
                                    .send(UiMessage::OpenQueryUrl(u.clone()))
                                    .unwrap();
                            });
                        } else {
                            app.pop_layer();
                        }
                    }),
            );
        }
        self.trigger();
    }

    fn open_gemini_query_dialog(&mut self, url: Url, query: String) {
        {
            let mut app = self.app.write().unwrap();
            app.add_layer(
                Dialog::new()
                    .title(query)
                    .content(
                        EditView::new()
                            // Call `show_popup` when the user presses `Enter`
                            //FIXME: create closure with url: .on_submit(search)
                            .with_name("query")
                            .fixed_width(30),
                    )
                    .button("Cancel", move |app| {
                        app.pop_layer();
                    })
                    .button("Ok", move |app| {
                        let name =
                            app.call_on_name("query", |view: &mut EditView| view.get_content());
                        if let Some(n) = name {
                            let mut u = url.clone();
                            u.set_query(Some(&n));
                            info!("open_gemini_query_dialog(): url = {}", u);

                            app.pop_layer(); // Close search dialog
                            app.with_user_data(|userdata: &mut UserData| {
                                userdata
                                    .ui_tx
                                    .write()
                                    .unwrap()
                                    .send(UiMessage::OpenUrlFromString(
                                        String::from(u.as_str()),
                                        true,
                                        0,
                                    ))
                                    .unwrap();
                            });
                        } else {
                            app.pop_layer();
                        }
                    }),
            );
        }
        self.trigger();
    }

    fn query(&mut self, url: Url) {
        trace!("query({});", url);
        self.set_message("Loading ...");
        self.controller_tx
            .read()
            .unwrap()
            .send(ControllerMessage::FetchUrl(url, ItemType::Dir, true, 0))
            .unwrap();
    }

    // Helper function to get the width of the view port
    // Used for text wrapping
    fn get_viewport_width(&mut self) -> usize {
        let mut app = self.app.write().unwrap();
        app.call_on_name("main", |v: &mut ui::layout::Layout| {
            v.set_view("content");
        });
        let mut width = 0;
        app.call_on_name(
            "content_scroll",
            |s: &mut ScrollView<ResizedView<NamedView<SelectView<GopherMapEntry>>>>| {
                let rect = s.content_viewport();
                width = rect.width();
            },
        );
        width
    }

    // Helper function to get the width of the gemini view port
    fn get_gemini_viewport_width(&mut self) -> usize {
        let mut app = self.app.write().unwrap();
        app.call_on_name("main", |v: &mut ui::layout::Layout| {
            v.set_view("gemini_content");
        });
        let mut width = 0;
        app.call_on_name(
            "gemini_content_scroll",
            |s: &mut ScrollView<ResizedView<NamedView<SelectView<GeminiLine>>>>| {
                let rect = s.content_viewport();
                width = rect.width();
            },
        );
        width
    }

    /*
    // Helper function to get the width of the view port
    // Used for text wrapping
    fn get_text_viewport_width(&mut self) -> usize {
        let mut app = self.app.write().unwrap();
        let mut width = 0;
        // FIXME: Does not call the closure for some reason
        app.call_on_name(
            "text_scroll",
            |s: &mut ScrollView<ResizedView<NamedView<SelectView>>>| {
                info!("TEXT_SCROLL");
                let rect = s.content_viewport();
                width = rect.width() - 2;
            },
        );
        width
    }
    */

    /// Renders a gemini site in a cursive::TextView
    fn show_gemini(&mut self, base_url: &Url, content: &str) {
        trace!("show_gemini()");
        let viewport_width = self.get_gemini_viewport_width() - 10;
        let mut app = self.app.write().unwrap();
        app.call_on_name("main", |v: &mut ui::layout::Layout| {
            v.set_view("gemini_content");
        });
        info!("Viewport-width = {}", viewport_width);
        app.call_on_name("gemini_content", |v: &mut SelectView<GeminiLine>| {
            v.clear();
            let lines = content.lines();
            for l in lines {
                let line = l.to_string();
                if let Ok(gemini_line) = GeminiLine::parse(line.clone(), &base_url) {
                    match gemini_line.line_type {
                        LineType::Text => {
                            let label = gemini_line.clone().label();
                            if label.len() > viewport_width {
                                let iter = wrap_iter(&label, viewport_width);
                                for str in iter {
                                    let mut formatted = StyledString::new();
                                    let label = format!("       {}", str);
                                    formatted.append(label);
                                    v.add_item(formatted, gemini_line.clone());
                                }
                            } else {
                                v.add_item(format!("       {}", label), gemini_line.clone());
                            }
                        }
                        LineType::UnorderedList => {
                            let label = gemini_line.clone().label();
                            if label.len() > viewport_width {
                                let iter = wrap_iter(&label, viewport_width);
                                let mut first = true;
                                for str in iter {
                                    let mut formatted = StyledString::new();
                                    let label = if first {
                                        format!("       {}", str)
                                    } else {
                                        format!("         {}", str)
                                    };
                                    first = false;
                                    formatted.append(label);
                                    v.add_item(formatted, gemini_line.clone());
                                }
                            } else {
                                v.add_item(format!("       {}", label), gemini_line.clone());
                            }
                        }
                        LineType::Link => {
                            let label = gemini_line.clone().label();
                            v.add_item(label, gemini_line.clone());
                        }
                        LineType::PreformattedToggle => {
                            // Skip for now
                        }
                        _ => {
                            let label = gemini_line.clone().label();
                            v.add_item(label, gemini_line.clone());
                        }
                    }
                }
            }
            v.set_on_submit(|app, entry| {
                app.with_user_data(|userdata: &mut UserData| {
                    if entry.line_type == LineType::Link {
                        let url = entry.url.as_ref().unwrap();
                        info!("Trying to open {}", url.as_str());
                        userdata
                            .ui_tx
                            .write()
                            .unwrap()
                            .send(UiMessage::OpenUrlFromString(
                                String::from(url.as_str()),
                                true,
                                0,
                            ))
                            .unwrap();
                    }
                });
            });
        });
    }

    /// Renders a gophermap in a cursive::TextView
    fn show_gophermap(&mut self, content: String, index: usize) {
        let mut title: String = "".to_string();
        let viewport_width = self.get_viewport_width() - 7;
        info!("Viewport-width = {}", viewport_width);
        let mut app = self.app.write().unwrap();
        app.call_on_name("main", |v: &mut ui::layout::Layout| {
            v.set_view("content");
        });
        let msg = String::new();

        app.call_on_name("content", |view: &mut SelectView<GopherMapEntry>| {
            view.clear();
            let lines = content.lines();
            let mut gophermap = Vec::new();
            let mut first = true;
            for l in lines {
                if first {
                    if l.starts_with('/') {
                        title = l.to_string();
                    }
                    first = false;
                }
                if l != "." {
                    match GopherMapEntry::parse(l.to_string()) {
                        Ok(gl) => {
                            gophermap.push(gl);
                        }
                        Err(err) => {
                            warn!("Invalid gophermap line: {}", err);
                        }
                    };
                }
            }
            for l in gophermap {
                let entry = l.clone();

                let label = entry.clone().label();
                if entry.item_type == ItemType::Inline && label.len() > viewport_width {
                    let iter = wrap_iter(&label, viewport_width);
                    // TODO: use cursive::utils::lines::simple::make_lines
                    for str in iter {
                        let mut formatted = StyledString::new();
                        let label = format!("{}  {}", ItemType::as_str(entry.item_type), str);
                        formatted.append(label);
                        view.add_item(formatted, l.clone());
                    }
                } else {
                    let mut formatted = StyledString::new();
                    let label = format!("{}  {}", ItemType::as_str(entry.item_type), entry.label());
                    formatted.append(label);
                    view.add_item(formatted, l.clone());
                }
            }
            view.set_on_submit(|app, entry| {
                app.with_user_data(|userdata: &mut UserData| {
                    // FIXME Remove duplicate code
                    if ItemType::is_download(entry.item_type)
                        || ItemType::is_text(entry.item_type)
                        || ItemType::is_dir(entry.item_type)
                    {
                        userdata
                            .ui_tx
                            .write()
                            .unwrap()
                            .send(UiMessage::OpenUrl(entry.url.clone(), true, 0))
                            .unwrap();
                    } else if ItemType::is_query(entry.item_type) {
                        userdata
                            .ui_tx
                            .write()
                            .unwrap()
                            .send(UiMessage::OpenQueryDialog(entry.url.clone()))
                            .unwrap();
                    } else if ItemType::is_html(entry.item_type) {
                        userdata
                            .controller_tx
                            .write()
                            .unwrap()
                            .send(ControllerMessage::OpenHtml(entry.url.clone()))
                            .unwrap();
                    } else if ItemType::is_image(entry.item_type) {
                        userdata
                            .controller_tx
                            .write()
                            .unwrap()
                            .send(ControllerMessage::OpenImage(entry.url.clone()))
                            .unwrap();
                    } else if ItemType::is_telnet(entry.item_type) {
                        userdata
                            .controller_tx
                            .write()
                            .unwrap()
                            .send(ControllerMessage::OpenTelnet(entry.url.clone()))
                            .unwrap();
                    }
                });
            });
            view.set_selection(index);
        });
        if !msg.is_empty() {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .ui_tx
                    .read()
                    .unwrap()
                    .send(UiMessage::ShowMessage(msg))
                    .unwrap()
            });
        }

        // FIXME: Call this from the previous callback
        if !title.is_empty() {
            trace!("TITLE SET");
            app.call_on_name("main", |v: &mut ui::layout::Layout| {
                trace!("SET TITLE");
                v.set_title("content".to_string(), title);
            });
        }
    }

    /// Renders a text file in a cursive::TextView
    fn show_text_file(&mut self, content: String) {
        let viewport_width = self.get_viewport_width() - 2;
        let mut app = self.app.write().unwrap();
        app.call_on_name("main", |v: &mut ui::layout::Layout| {
            v.set_view("text");
        });
        app.call_on_name("text", |v: &mut SelectView| {
            v.clear();
            let lines = content.lines();
            for l in lines {
                let line = l.to_string();
                if line.len() > viewport_width {
                    let iter = wrap_iter(&line, viewport_width);
                    info!("Wrapping text");
                    for str in iter {
                        v.add_item_str(format!("  {}", str));
                    }
                } else {
                    v.add_item_str(format!("  {}", line));
                }
            }
            // TODO: on_submit-handler to open URLs in text
        });
    }

    fn show_certificate_changed_dialog(&mut self, url: Url, fingerprint: String) {
        {
            let mut app = self.app.write().unwrap();
            app.add_layer(
                Dialog::new()
                    .title("Certificate warning")
                    .content(TextView::new(format!("The certificate for the following URL has changed:\n{}\nDo you want to continue?", url.as_str())))
                    .button("Cancel", |app| {
                        app.pop_layer(); // Close edit bookmark
                    })
                    .button("Accept the risk", move |app| {
                        app.pop_layer(); // Close edit bookmark

                        app.with_user_data(|userdata: &mut UserData| {
                            userdata
                                .controller_tx
                                .read()
                                .unwrap()
                                .clone()
                                .send(ControllerMessage::UpdateCertificateAndOpen(url.clone(), fingerprint.clone())
                                )
                                .unwrap()
                        });
                    })
            );
        }
        self.trigger();
    }

    fn show_add_bookmark_dialog(&mut self, bookmark: Bookmark) {
        {
            let mut app = self.app.write().unwrap();
            let url = bookmark.url;
            let title = bookmark.title;
            let tags = bookmark.tags;
            app.add_layer(
                Dialog::new()
                    .title("Add Bookmark")
                    .content(
                        LinearLayout::vertical()
                            .child(TextView::new("URL:"))
                            .child(
                                EditView::new()
                                    .content(url.into_string().as_str())
                                    .with_name("url")
                                    .fixed_width(30),
                            )
                            .child(TextView::new("\nTitle:"))
                            .child(
                                EditView::new()
                                    .content(title.as_str())
                                    .with_name("title")
                                    .fixed_width(30),
                            )
                            .child(TextView::new("Tags (comma separated):"))
                            .child(
                                EditView::new()
                                    .content(tags.join(",").as_str())
                                    .with_name("tags")
                                    .fixed_width(30),
                            ),
                    )
                    .button("Ok", move |app| {
                        let url = app
                            .call_on_name("url", |view: &mut EditView| view.get_content())
                            .unwrap();
                        let title = app
                            .call_on_name("title", |view: &mut EditView| view.get_content())
                            .unwrap();
                        let tags = app
                            .call_on_name("tags", |view: &mut EditView| view.get_content())
                            .unwrap();

                        // Validate URL
                        if let Ok(_url) = Url::parse(&url) {
                            app.pop_layer(); // Close edit bookmark
                            app.with_user_data(|userdata: &mut UserData| {
                                userdata
                                    .controller_tx
                                    .read()
                                    .unwrap()
                                    .clone()
                                    .send(ControllerMessage::AddBookmark(
                                        _url.clone(),
                                        title.to_string(),
                                        tags.to_string(),
                                    ))
                                    .unwrap()
                            });
                        } else {
                            app.add_layer(Dialog::info("Invalid URL!"));
                        }
                    })
                    .button("Cancel", |app| {
                        app.pop_layer(); // Close edit bookmark
                    }),
            );
        }
        self.trigger();
    }

    fn show_edit_history_dialog(&mut self, entries: Vec<HistoryEntry>) {
        let mut view: SelectView<HistoryEntry> = SelectView::new();
        for e in entries {
            let mut url: String = format!("{:<20}", e.url.clone().as_str());
            url.truncate(50);
            view.add_item(
                format!(
                    "{:>4} {:<20} {}",
                    e.visited_count,
                    e.timestamp.format("%Y-%m-%d %H:%M:%S"),
                    url
                ),
                e,
            );
        }
        {
            let mut app = self.app.write().unwrap();
            app.add_layer(
                Dialog::new()
                    .title("Show history")
                    .content(
                        LinearLayout::vertical()
                            .child(TextView::new("#Vis Last Visited         URL"))
                            .child(
                                LinearLayout::vertical()
                                    .child(view.with_name("entries").scrollable()),
                            ),
                    )
                    .button("Clear all history", move |app| {
                        app.add_layer(
                            Dialog::around(TextView::new("Do you want to delete the history?"))
                                .button("Cancel", |app| {
                                    app.pop_layer();
                                })
                                .button("Ok", |app| {
                                    app.pop_layer();
                                    app.call_on_name(
                                        "entries",
                                        |view: &mut SelectView<HistoryEntry>| view.clear(),
                                    );
                                    app.with_user_data(|userdata: &mut UserData| {
                                        userdata
                                            .controller_tx
                                            .read()
                                            .unwrap()
                                            .send(ControllerMessage::ClearHistory)
                                            .unwrap()
                                    });
                                }),
                        );
                    })
                    /*
                    ??
                    .button("Clear last hour", move |app| {
                    })
                    */
                    .button("Open URL", move |app| {
                        let selected = app
                            .call_on_name("entries", |view: &mut SelectView<HistoryEntry>| {
                                view.selection()
                            })
                            .unwrap();
                        app.pop_layer();
                        match selected {
                            None => (),
                            Some(b) => {
                                app.with_user_data(|userdata: &mut UserData| {
                                    userdata
                                        .ui_tx
                                        .read()
                                        .unwrap()
                                        .clone()
                                        .send(UiMessage::OpenUrlFromString(
                                            b.url.to_string(),
                                            true,
                                            0,
                                        ))
                                        .unwrap()
                                });
                            }
                        }
                    })
                    .button("Close", |app| {
                        app.pop_layer(); // Close dialog
                    }),
            );
        }
        self.trigger();
    }

    fn show_search_dialog(&mut self, url: Url) {
        let ui_tx_clone = self.ui_tx.read().unwrap().clone();
        {
            let mut app = self.app.write().unwrap();
            let queryurl = url.clone();
            app.add_layer(
                Dialog::new()
                    .title("Enter search term")
                    .content(
                        EditView::new()
                            .on_submit(move |app, query| {
                                app.pop_layer();
                                let mut u = queryurl.clone();
                                let mut path = u.path().to_string();
                                path.push_str("%09");
                                path.push_str(&query);
                                u.set_path(path.as_str());
                                info!("show_search_dialog(): url = {}", u);
                                app.with_user_data(|userdata: &mut UserData| {
                                    userdata
                                        .ui_tx
                                        .read()
                                        .unwrap()
                                        .send(UiMessage::OpenQueryUrl(u))
                                        .unwrap()
                                });
                            })
                            .with_name("search")
                            .fixed_width(30),
                    )
                    .button("Cancel", move |app| {
                        app.pop_layer();
                    })
                    .button("Ok", move |app| {
                        let name =
                            app.call_on_name("search", |view: &mut EditView| view.get_content());
                        if let Some(n) = name {
                            app.pop_layer();
                            let mut u = url.clone();
                            let mut path = u.path().to_string();
                            path.push_str("%09");
                            path.push_str(&n);
                            u.set_path(path.as_str());
                            info!("show_search_dialog(): url = {}", u);
                            ui_tx_clone.send(UiMessage::OpenQueryUrl(u)).unwrap();
                        } else {
                            app.pop_layer(); // Close search dialog
                            app.add_layer(Dialog::info("No search parameter!"))
                        }
                    }),
            );
        }
        self.trigger();
    }

    pub fn show_settings_dialog(&mut self) {
        let download_path = SETTINGS.read().unwrap().get_str("download_path").unwrap();
        let homepage_url = SETTINGS.read().unwrap().get_str("homepage").unwrap();
        let theme = SETTINGS.read().unwrap().get_str("theme").unwrap();
        let html_command = SETTINGS.read().unwrap().get_str("html_command").unwrap();
        let image_command = SETTINGS.read().unwrap().get_str("image_command").unwrap();
        let telnet_command = SETTINGS.read().unwrap().get_str("telnet_command").unwrap();
        let darkmode = theme == "darkmode";
        {
            let mut app = self.app.write().unwrap();
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
                            .child(Checkbox::new().with_name("darkmode")
                            )
                    )
                    .button("Cancel", |app| {
                        app.pop_layer();
                    })
                    .button("Apply",  |app| {
                        let homepage = app.call_on_name("homepage", |view: &mut EditView| {
                            view.get_content()
                        }).unwrap();
                        let download = app.call_on_name("download_path", |view: &mut EditView| {
                            view.get_content()
                        }).unwrap();
                        let darkmode = app.call_on_name("darkmode", |view: &mut Checkbox| {
                            view.is_checked()
                        }).unwrap();
                        let html_command = app.call_on_name("html_command", |view: &mut EditView| {
                            view.get_content()
                        }).unwrap();
                        let image_command = app.call_on_name("image_command", |view: &mut EditView| {
                            view.get_content()
                        }).unwrap();
                        let telnet_command = app.call_on_name("telnet_command", |view: &mut EditView| {
                            view.get_content()
                        }).unwrap();
                        app.pop_layer();
                        if let Ok(_url) = Url::parse(&homepage) {
                            SETTINGS.write().unwrap().set::<String>("homepage", homepage.clone().to_string()).unwrap();
                            SETTINGS.write().unwrap().set::<String>("download_path", download.to_string()).unwrap();
                            SETTINGS.write().unwrap().set::<String>("html_command", html_command.to_string()).unwrap();
                            SETTINGS.write().unwrap().set::<String>("image_command", image_command.to_string()).unwrap();
                            SETTINGS.write().unwrap().set::<String>("telnet_command", telnet_command.to_string()).unwrap();
                            let theme = if darkmode { "darkmode" } else { "lightmode" };
                            app.load_toml(SETTINGS.read().unwrap().get_theme_by_name(theme.to_string())).unwrap();
                            SETTINGS.write().unwrap().set::<String>("theme", theme.to_string()).unwrap();

                            if let Err(why) = SETTINGS.write().unwrap().write_settings_to_file() {
                                app.add_layer(Dialog::info(format!("Could not write config file: {}", why)));
                            }
                        } else {
                            app.add_layer(Dialog::info("Invalid homepage url"));
                        }
                        app.with_user_data(|userdata: &mut UserData|
                                           userdata.ui_tx.read().unwrap()
                                           .send(UiMessage::Trigger).unwrap()
                        );
                    }),
            );
            app.call_on_name("darkmode", |view: &mut Checkbox| {
                if darkmode {
                    view.check();
                } else {
                    view.uncheck();
                }
            })
            .unwrap();
        }
        self.trigger();
    }

    pub fn show_url_dialog(&mut self) {
        {
            let mut app = self.app.write().unwrap();
            app.add_layer(
                Dialog::new()
                    .title("Enter gopher or gemini URL:")
                    .content(
                        EditView::new()
                            .on_submit(NcGopher::open_url_action)
                            .with_name("name")
                            .fixed_width(50),
                    )
                    .button("Cancel", move |app| {
                        app.pop_layer();
                    })
                    .button("Ok", |app| {
                        let name = app
                            .call_on_name("name", |view: &mut EditView| view.get_content())
                            .unwrap();
                        NcGopher::open_url_action(app, name.as_str())
                    }),
            );
        } // drop lock on app before calling trigger:
        self.trigger();
    }

    fn open_url_action(app: &mut Cursive, name: &str) {
        app.pop_layer();
        app.with_user_data(|userdata: &mut UserData| {
            userdata
                .ui_tx
                .read()
                .unwrap()
                .send(UiMessage::OpenUrlFromString(name.to_string(), true, 0))
                .unwrap()
        });
    }

    /// If the cursor in the current view is on a link, show
    /// a status message in the statusbar.
    fn show_current_link_info(&mut self) {
        let mut current_view = String::new();
        {
            let mut app = self.app.write().expect("Could not get write lock on app");
            app.call_on_name("main", |v: &mut ui::layout::Layout| {
                if let Some(v) = v.get_current_view() {
                    current_view = v;
                }
            });
        }
        match current_view.as_str() {
            "content" => self.show_current_link_info_gopher(),
            "gemini_content" => self.show_current_link_info_gemini(),
            _ => (),
        }
    }

    fn show_current_link_info_gemini(&mut self) {
        let mut app = self.app.write().expect("Could not get write lock on app");
        let view: ViewRef<SelectView<GeminiLine>>;
        if let Some(v) = app.find_name("gemini_content") {
            view = v;
        } else {
            return;
        }
        let cur = match view.selected_id() {
            Some(id) => id,
            None => 0,
        };
        if let Some((_, item)) = view.get_item(cur) {
            if let Some(url) = &item.url {
                app.with_user_data(|userdata: &mut UserData| {
                    userdata
                        .ui_tx
                        .read()
                        .unwrap()
                        .send(UiMessage::ShowMessage(format!("URL '{}'", url)))
                        .unwrap()
                });
            }
        }
    }

    fn show_current_link_info_gopher(&mut self) {
        let mut app = self.app.write().expect("Could not get write lock on app");
        let view: ViewRef<SelectView<GopherMapEntry>>;
        if let Some(v) = app.find_name("content") {
            view = v;
        } else {
            return;
        }
        let cur = match view.selected_id() {
            Some(id) => id,
            None => 0,
        };
        if let Some((_, item)) = view.get_item(cur) {
            match item.item_type {
                ItemType::Html => {
                    let mut url = item.url.clone().into_string();
                    if url.starts_with("URL:") {
                        url.replace_range(..3, "");
                        app.with_user_data(|userdata: &mut UserData| {
                            userdata
                                .ui_tx
                                .read()
                                .unwrap()
                                .send(UiMessage::ShowMessage(format!("URL '{}'", url)))
                                .unwrap()
                        });
                    } else {
                        app.with_user_data(|userdata: &mut UserData| {
                            userdata
                                .ui_tx
                                .read()
                                .unwrap()
                                .send(UiMessage::ShowMessage(format!("URL '{}'", url)))
                                .unwrap()
                        });
                    }
                }
                ItemType::Inline => (),
                _ => {
                    app.with_user_data(|userdata: &mut UserData| {
                        userdata
                            .ui_tx
                            .read()
                            .unwrap()
                            .send(UiMessage::ShowMessage(format!("URL '{}'", item.url)))
                            .unwrap()
                    });
                }
            }
        };
    }

    pub fn get_selected_item_index(&self) -> Option<usize> {
        let mut app = self.app.write().expect("Could not get read lock on app");
        let view: ViewRef<SelectView<GopherMapEntry>>;
        if let Some(v) = app.find_name("content") {
            view = v;
        } else {
            warn!("Could not find content");
            return None;
        }
        let cur = match view.selected_id() {
            Some(id) => id,
            None => 0,
        };
        let i: usize = cur;
        warn!("get_selected_item_index() => {}", i);
        Some(i)
    }

    fn move_selection(&mut self, dir: Direction) {
        trace!("move_selection({:?})", dir);
        let mut app = self.app.write().expect("Could not get write lock on app");
        let mut current_view = String::new();
        app.call_on_name("main", |v: &mut ui::layout::Layout| {
            if let Some(v) = v.get_current_view() {
                current_view = v;
            }
        });
        match current_view.as_str() {
            "content" => {
                let mut view: ViewRef<SelectView<GopherMapEntry>>;
                if let Some(v) = app.find_name("content") {
                    view = v;
                    let callback = match dir {
                        Direction::Next => view.select_down(1),
                        Direction::Previous => view.select_up(1),
                    };
                    callback(&mut app);
                    if let Some(id) = view.selected_id() {
                        app.call_on_name(
                            "content_scroll",
                            |s: &mut ScrollView<
                                ResizedView<NamedView<SelectView<GopherMapEntry>>>,
                            >| {
                                s.set_offset(cursive::Vec2::new(0, id));
                            },
                        );
                    }
                }
            }
            "gemini_content" => {
                let mut view: ViewRef<SelectView<GeminiLine>>;
                if let Some(v) = app.find_name("gemini_content") {
                    view = v;
                    let callback = match dir {
                        Direction::Next => view.select_down(1),
                        Direction::Previous => view.select_up(1),
                    };
                    callback(&mut app);
                    if let Some(id) = view.selected_id() {
                        app.call_on_name(
                            "gemini_content_scroll",
                            |s: &mut ScrollView<ResizedView<NamedView<SelectView<GeminiLine>>>>| {
                                s.set_offset(cursive::Vec2::new(0, id));
                            },
                        );
                    }
                }
            }
            _ => (),
        }
        app.with_user_data(|userdata: &mut UserData| {
            userdata
                .ui_tx
                .read()
                .unwrap()
                .send(UiMessage::Trigger)
                .unwrap()
        });
    }

    fn move_to_link(&mut self, dir: Direction) {
        let mut current_view = String::new();
        {
            let mut app = self.app.write().expect("Could not get write lock on app");
            app.call_on_name("main", |v: &mut ui::layout::Layout| {
                if let Some(v) = v.get_current_view() {
                    current_view = v;
                }
            });
        }
        match current_view.as_str() {
            "content" => self.move_to_link_gopher(dir),
            "gemini_content" => self.move_to_link_gemini(dir),
            _ => (),
        }
    }

    fn move_to_link_gemini(&mut self, dir: Direction) {
        let mut app = self.app.write().expect("Could not get write lock on app");
        let mut view: ViewRef<SelectView<GeminiLine>>;
        if let Some(v) = app.find_name("gemini_content") {
            view = v;
        } else {
            return;
        }
        let cur = match view.selected_id() {
            Some(id) => id,
            None => 0,
        };
        let mut i: usize = cur;
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
                    if item.line_type == LineType::Link {
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
                    if item.line_type == LineType::Link {
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
        app.call_on_name(
            "gemini_content_scroll",
            |s: &mut ScrollView<ResizedView<NamedView<SelectView<GeminiLine>>>>| {
                s.set_offset(cursive::Vec2::new(0, selected_id));
            },
        );
        app.with_user_data(|userdata: &mut UserData| {
            userdata
                .ui_tx
                .read()
                .unwrap()
                .send(UiMessage::Trigger)
                .unwrap()
        });
    }

    fn move_to_link_gopher(&mut self, dir: Direction) {
        let mut app = self.app.write().expect("Could not get write lock on app");
        let mut view: ViewRef<SelectView<GopherMapEntry>>;
        if let Some(v) = app.find_name("content") {
            view = v;
        } else {
            return;
        }
        let cur = match view.selected_id() {
            Some(id) => id,
            None => 0,
        };
        let mut i: usize = cur;
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
                    if !ItemType::is_inline(item.item_type) {
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
                    if !ItemType::is_inline(item.item_type) {
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
        app.call_on_name(
            "content_scroll",
            |s: &mut ScrollView<ResizedView<NamedView<SelectView<GopherMapEntry>>>>| {
                s.set_offset(cursive::Vec2::new(0, selected_id));
            },
        );
        app.with_user_data(|userdata: &mut UserData| {
            userdata
                .ui_tx
                .read()
                .unwrap()
                .send(UiMessage::Trigger)
                .unwrap()
        });
    }

    fn show_edit_bookmarks_dialog(&mut self, bookmarks: Vec<Bookmark>) {
        let mut view: SelectView<Bookmark> = SelectView::new();
        for b in bookmarks {
            let mut title: String = format!("{:<20}", b.title.clone().as_str());
            title.truncate(20);
            let mut url = format!("{:<50}", b.url.clone().as_str());
            url.truncate(50);
            view.add_item(format!("{} | {}", title, url), b);
        }
        {
            let mut app = self.app.write().unwrap();
            app.add_layer(
                Dialog::new()
                    .title("Edit bookmarks")
                    .content(
                        LinearLayout::vertical().child(view.with_name("bookmarks").scrollable()),
                    )
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
                                let bm = Bookmark {
                                    title: (*bookmark).title.clone(),
                                    url: (*bookmark).url.clone(),
                                    tags: Vec::new(),
                                };
                                app.with_user_data(|userdata: &mut UserData| {
                                    userdata
                                        .controller_tx
                                        .read()
                                        .unwrap()
                                        .send(ControllerMessage::RemoveBookmark(bm))
                                        .unwrap()
                                });
                            }
                        }
                    })
                    .button("Open", |app| {
                        let selected = app
                            .call_on_name("bookmarks", |view: &mut SelectView<Bookmark>| {
                                view.selection()
                            })
                            .unwrap();
                        match selected {
                            None => (),
                            Some(b) => {
                                app.with_user_data(|userdata: &mut UserData| {
                                    userdata
                                        .ui_tx
                                        .read()
                                        .unwrap()
                                        .clone()
                                        .send(UiMessage::OpenUrlFromString(
                                            b.url.to_string(),
                                            true,
                                            0,
                                        ))
                                        .unwrap()
                                });
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
                                let bookmark = Bookmark {
                                    url: b.url.clone(),
                                    title: b.title.clone(),
                                    tags: b.tags.clone(),
                                };
                                app.pop_layer();
                                app.with_user_data(|userdata: &mut UserData| {
                                    userdata
                                        .ui_tx
                                        .read()
                                        .unwrap()
                                        .send(UiMessage::ShowAddBookmarkDialog(bookmark))
                                        .unwrap()
                                });
                            }
                        }
                    })
                    .button("Close", |app| {
                        app.pop_layer();
                    }),
            );
        }
        self.trigger();
    }

    fn show_save_as_dialog(&mut self, url: Url) {
        {
            let mut filename = self.get_filename_from_url(url);
            if filename.is_empty() {
                filename.push_str("download");
            }
            if !filename.ends_with(".txt") {
                filename.push_str(".txt");
            }
            let mut app = self.app.write().unwrap();
            app.add_layer(
                Dialog::new()
                    .title("Enter filename:")
                    .content(
                        EditView::new()
                            .on_submit(NcGopher::save_as_action)
                            .with_name("name")
                            .fixed_width(50),
                    )
                    .button("Cancel", move |app| {
                        app.pop_layer();
                    })
                    .button("Ok", move |app| {
                        let name = app
                            .call_on_name("name", |view: &mut EditView| view.get_content())
                            .unwrap();
                        NcGopher::save_as_action(app, name.as_str())
                    }),
            );
            app.call_on_name("name", |v: &mut EditView| {
                v.set_content(filename);
            });
        }
        self.trigger();
    }

    fn save_as_action(app: &mut Cursive, name: &str) {
        app.pop_layer();
        if !name.is_empty() {
            app.with_user_data(|userdata: &mut UserData| {
                userdata
                    .controller_tx
                    .read()
                    .unwrap()
                    .send(ControllerMessage::SavePageAs(name.to_string()))
                    .unwrap()
            });
        } else {
            app.add_layer(Dialog::info("No filename given!"))
        }
    }

    fn add_to_bookmark_menu(&mut self, b: Bookmark) {
        info!("add_to_bookmark_menu()");
        let mut app = self.app.write().unwrap();
        let menutree = app.menubar().find_subtree("Bookmarks");
        if let Some(tree) = menutree {
            let b2 = b.clone();
            tree.insert_leaf(3, b.title.as_str(), move |app| {
                info!("Adding bm to bookmark menu");
                let b3 = b2.clone();
                app.with_user_data(|userdata: &mut UserData| {
                    userdata
                        .ui_tx
                        .read()
                        .unwrap()
                        .clone()
                        .send(UiMessage::OpenUrlFromString(b3.url.to_string(), true, 0))
                        .unwrap()
                });
            });
        }
    }

    fn add_to_history_menu(&mut self, h: HistoryEntry) {
        const HISTORY_LEN: usize = 10;
        let mut app = self.app.write().unwrap();
        let menutree = app.menubar().find_subtree("History");
        if let Some(tree) = menutree {
            // Add 3 to account for the two first menuitems + separator
            if tree.len() > HISTORY_LEN + 3 {
                tree.remove(tree.len() - 1);
            }
            // TODO: Refactor.
            // There must be a more ideomatic way than h->h2->h3
            let h2 = h.clone();
            tree.insert_leaf(3, h.title.as_str(), move |app| {
                let h3 = h2.clone();
                app.with_user_data(|userdata: &mut UserData| {
                    userdata
                        .ui_tx
                        .read()
                        .unwrap()
                        .clone()
                        .send(UiMessage::OpenUrlFromString(h3.url.to_string(), true, 0))
                        .unwrap()
                });
            });
        }
    }

    fn clear_history_menu(&mut self) {
        let mut app = self.app.write().unwrap();
        let menutree = app.menubar().find_subtree("History");
        if let Some(tree) = menutree {
            while tree.len() > 3 {
                tree.remove(tree.len() - 1);
            }
        }
    }

    fn clear_bookmarks_menu(&mut self) {
        let mut app = self.app.write().unwrap();
        let menutree = app.menubar().find_subtree("Bookmarks");
        if let Some(tree) = menutree {
            while tree.len() > 3 {
                tree.remove(tree.len() - 1);
            }
        }
    }

    /// Triggers a rerendring of the UI
    pub fn trigger(&self) {
        info!("Trigger");
        // send a no-op to trigger event loop processing
        let app = self.app.read().unwrap();
        app.cb_sink()
            .send(Box::new(Cursive::noop))
            .expect("could not send no-op event to cursive");
    }

    /// Step the UI by calling into Cursive's step function, then
    /// processing any UI messages.
    pub fn step(&mut self) -> bool {
        {
            if !self.is_running {
                return false;
            }
        }

        // Process any pending UI messages
        while let Some(message) = self.ui_rx.try_iter().next() {
            match message {
                UiMessage::AddToBookmarkMenu(bookmark) => {
                    self.add_to_bookmark_menu(bookmark);
                }
                UiMessage::AddToHistoryMenu(history_entry) => {
                    self.add_to_history_menu(history_entry);
                }
                UiMessage::BinaryWritten(filename, bytes_written) => {
                    self.binary_written(filename, bytes_written);
                }
                UiMessage::ClearHistoryMenu => {
                    self.clear_history_menu();
                }
                UiMessage::ClearBookmarksMenu => {
                    self.clear_bookmarks_menu();
                }
                UiMessage::PageSaved(_url, filename) => {
                    self.set_message(format!("Page saved as '{}'.", filename).as_str());
                }
                UiMessage::ShowAddBookmarkDialog(bookmark) => {
                    self.show_add_bookmark_dialog(bookmark);
                }
                UiMessage::ShowEditHistoryDialog(entries) => {
                    self.show_edit_history_dialog(entries);
                }
                UiMessage::ShowContent(url, content, item_type, index) => {
                    if ItemType::is_dir(item_type) {
                        self.show_gophermap(content, index);
                    } else if ItemType::is_text(item_type) {
                        self.show_text_file(content);
                    }
                    self.set_message(url.as_str());
                }
                UiMessage::ShowCertificateChangedDialog(url, fingerprint) => {
                    self.show_certificate_changed_dialog(url, fingerprint);
                }
                UiMessage::ShowGeminiContent(url, gemini_type, content) => {
                    if gemini_type == GeminiType::Text {
                        self.show_text_file(content);
                    } else {
                        self.show_gemini(&url, &content);
                    }
                    self.set_message(url.as_str());
                }
                UiMessage::MoveSelection(direction) => {
                    self.move_selection(direction);
                }
                UiMessage::MoveToLink(direction) => {
                    self.move_to_link(direction);
                }
                UiMessage::OpenQueryDialog(url) => {
                    self.open_query_dialog(url);
                }
                UiMessage::OpenGeminiQueryDialog(url, query) => {
                    self.open_gemini_query_dialog(url, query);
                }
                UiMessage::OpenQueryUrl(url) => {
                    self.query(url);
                }
                UiMessage::OpenUrl(url, add_to_history, index) => {
                    trace!("OpenUrl({}, {}, {})", url, add_to_history, index);
                    self.open_url(url, add_to_history, index);
                }
                UiMessage::OpenUrlFromString(url, add_to_history, index) => {
                    self.open_url_string(url, add_to_history, index);
                }
                // Exit the event loop
                UiMessage::Quit => self.is_running = false,
                UiMessage::ShowEditBookmarksDialog(bookmarks) => {
                    self.show_edit_bookmarks_dialog(bookmarks)
                }
                UiMessage::ShowLinkInfo => self.show_current_link_info(),
                UiMessage::ShowMessage(msg) => {
                    self.set_message(msg.as_str());
                }
                UiMessage::ShowURLDialog => {
                    self.show_url_dialog();
                }
                UiMessage::ShowSaveAsDialog(url) => {
                    self.show_save_as_dialog(url);
                }
                UiMessage::ShowSearchDialog(url) => {
                    self.show_search_dialog(url);
                }
                UiMessage::ShowSettingsDialog => {
                    self.show_settings_dialog();
                }
                UiMessage::Trigger => {
                    self.trigger();
                }
            }
        }

        // Step the UI
        let mut app = self.app.write().unwrap();
        app.step();

        true
    }
}
