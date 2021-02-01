use std::collections::HashMap;

use cursive::align::HAlign;
use cursive::direction::Direction;
use cursive::event::{AnyCb, Event, EventResult};
use cursive::theme::ColorStyle;
use cursive::traits::View;
use cursive::vec::Vec2;
use cursive::view::{IntoBoxedView, Selector};
use cursive::Printer;
use unicode_width::UnicodeWidthStr;

struct Screen {
    title: String,
    view: Box<dyn View>,
}

pub struct Layout {
    views: HashMap<String, Screen>,
    stack: Vec<Screen>,
    statusbar: Box<dyn View>,
    focus: Option<String>,
    screenchange: bool,
    last_size: Vec2,
    //    theme: Theme,
}

impl Layout {
    pub fn new<T: IntoBoxedView>(status: T /*, theme: Theme*/) -> Layout {
        Layout {
            views: HashMap::new(),
            stack: Vec::new(),
            statusbar: status.into_boxed_view(),
            focus: None,
            screenchange: true,
            last_size: Vec2::new(0, 0),
            // theme,
        }
    }

    pub fn add_view<S: Into<String>, T: IntoBoxedView>(&mut self, id: S, view: T, title: S) {
        let s = id.into();
        let screen = Screen {
            title: title.into(),
            view: view.into_boxed_view(),
        };
        self.views.insert(s.clone(), screen);
        self.focus = Some(s);
    }

    pub fn view<S: Into<String>, T: IntoBoxedView>(mut self, id: S, view: T, title: S) -> Self {
        (&mut self).add_view(id, view, title);
        self
    }

    pub fn set_view<S: Into<String>>(&mut self, id: S) {
        let s = id.into();
        self.focus = Some(s);
        self.screenchange = true;
        self.stack.clear();
    }

    pub fn set_title(&mut self, id: String, title: String) {
        if let Some(view) = self.views.get_mut(&id) {
            view.title = title;
        }
    }

    fn get_current_screen(&self) -> &Screen {
        if !self.stack.is_empty() {
            self.stack.last().unwrap()
        } else {
            let id = self.get_current_view();
            self.views
                .get(&id)
                .unwrap_or_else(|| panic!("View {} missing", id))
        }
    }

    pub fn get_current_view(&self) -> String {
        self.focus
            .as_ref()
            .cloned()
            .expect("Layout loaded without views")
    }

    fn get_current_screen_mut(&mut self) -> &mut Screen {
        if !self.stack.is_empty() {
            self.stack.last_mut().unwrap()
        } else {
            let id = self.get_current_view();
            self.views
                .get_mut(&id)
                .unwrap_or_else(|| panic!("View {} missing", id))
        }
    }
}

impl View for Layout {
    fn draw(&self, printer: &Printer<'_, '_>) {
        let screen = self.get_current_screen();
        // screen title
        printer.with_color(ColorStyle::title_primary(), |printer| {
            let offset = HAlign::Center.get_offset(screen.title.width(), printer.size.x);
            printer.print((offset, 0), &screen.title);

            if !self.stack.is_empty() {
                printer.print((1, 0), "<");
            }
        });

        // screen content
        screen.view.draw(
            &printer
                .offset((0, 1))
                .cropped((printer.size.x, printer.size.y - 3))
                .focused(true),
        );

        self.statusbar
            .draw(&printer.offset((0, printer.size.y - 2)));
    }

    fn layout(&mut self, size: Vec2) {
        self.last_size = size;

        self.statusbar.layout(Vec2::new(size.x, 2));

        self.get_current_screen_mut()
            .view
            .layout(Vec2::new(size.x, size.y - 3));

        // the focus view has changed, let the views know so they can redraw
        // their items
        if self.screenchange {
            self.screenchange = false;
        }
    }

    fn required_size(&mut self, constraint: Vec2) -> Vec2 {
        Vec2::new(constraint.x, constraint.y)
    }

    fn on_event(&mut self, event: Event) -> EventResult {
        if let Event::Mouse { position, .. } = event {
            if position.y < self.last_size.y.saturating_sub(2) {
                if let Some(ref id) = self.focus {
                    let screen = self.views.get_mut(id).unwrap();
                    screen.view.on_event(event.relativized(Vec2::new(0, 1)));
                }
            } else if position.y < self.last_size.y {
                self.statusbar
                    .on_event(event.relativized(Vec2::new(0, self.last_size.y - 2)));
            }

            EventResult::Consumed(None)
        } else {
            self.get_current_screen_mut().view.on_event(event)
        }
    }

    fn call_on_any<'a>(&mut self, s: &Selector, c: AnyCb<'a>) {
        if let Selector::Name("statusbar") = s {
            self.statusbar.call_on_any(s, c);
        } else {
            self.get_current_screen_mut().view.call_on_any(s, c)
        }
    }

    fn take_focus(&mut self, source: Direction) -> bool {
        self.get_current_screen_mut().view.take_focus(source)
    }
}
