use std::collections::HashMap;

use cursive::align::HAlign;
use cursive::direction::Direction;
use cursive::event::{AnyCb, Event, EventResult};
use cursive::theme::ColorStyle;
use cursive::traits::View;
use cursive::vec::Vec2;
use cursive::view::{IntoBoxedView, Selector};
use cursive::views::EditView;
use cursive::Printer;
use unicode_width::UnicodeWidthStr;

//use command::Command;
//use commands::CommandResult;
//use events;

struct Screen {
    title: String,
    view: Box<dyn View>,
}

pub struct Layout {
    views: HashMap<String, Screen>,
    stack: Vec<Screen>,
    statusbar: Box<dyn View>,
    focus: Option<String>,
    pub cmdline: EditView,
    cmdline_focus: bool,
    //    result: Result<Option<String>, String>,
    //    result_time: Option<SystemTime>,
    screenchange: bool,
    last_size: Vec2,
    //    ev: events::EventManager,
    //    theme: Theme,
}

impl Layout {
    pub fn new<T: IntoBoxedView>(status: T, /*ev: &events::EventManager, theme: Theme*/) -> Layout {
        let style = ColorStyle::new(
            //           ColorType::Color(*theme.palette.custom("cmdline_bg").unwrap()),
            //           ColorType::Color(*theme.palette.custom("cmdline").unwrap()),
            ColorStyle::secondary().front,
            ColorStyle::secondary().back,
        );

        Layout {
            views: HashMap::new(),
            stack: Vec::new(),
            statusbar: status.as_boxed_view(),
            focus: None,
            cmdline: EditView::new().filler(" ").style(style),
            cmdline_focus: false,
            //            result: Ok(None),
            //            result_time: None,
            screenchange: true,
            last_size: Vec2::new(0, 0),
            //            ev: ev.clone(),
            //            theme,
        }
    }

    pub fn add_view<S: Into<String>, T: IntoBoxedView>(&mut self, id: S, view: T, title: S) {
        let s = id.into();
        let screen = Screen {
            title: title.into(),
            view: view.as_boxed_view(),
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
        self.cmdline_focus = false;
        self.screenchange = true;
        self.stack.clear();
    }

    pub fn set_title(&mut self, id: String, title: String) {
        warn!("set_title({}, {}", id, title);
        if let Some(view) = self.views.get_mut(&id) {
            view.title = title;
        }
    }

    fn get_current_screen(&self) -> Option<&Screen> {
        if !self.stack.is_empty() {
            return self.stack.last();
        }

        if let Some(id) = self.focus.as_ref() {
            self.views.get(id)
        } else {
            None
        }
    }

    pub fn get_current_view(&self) -> Option<String> {
        if let Some(id) = self.focus.as_ref() {
            Some(id.to_string())
        } else {
            None
        }
    }

    fn get_current_screen_mut(&mut self) -> Option<&mut Screen> {
        if !self.stack.is_empty() {
            return self.stack.last_mut();
        }

        if let Some(id) = self.focus.as_ref() {
            self.views.get_mut(id)
        } else {
            None
        }
    }
}

impl View for Layout {
    fn draw(&self, printer: &Printer<'_, '_>) {
        //        let result = self.get_result();

        let cmdline_visible = self.cmdline.get_content().len() > 0;
        let cmdline_height = if cmdline_visible { 1 } else { 0 };
        //        if result.as_ref().map(Option::is_some).unwrap_or(true) {
        //            cmdline_height += 1;
        //        }

        if let Some(screen) = self.get_current_screen() {
            // screen title
            printer.with_color(ColorStyle::title_primary(), |printer| {
                let offset = HAlign::Center.get_offset(screen.title.width(), printer.size.x);
                printer.print((offset, 0), &screen.title);

                if !self.stack.is_empty() {
                    printer.print((1, 0), "<");
                }
            });

            // screen content
            let printer = &printer
                .offset((0, 1))
                .cropped((printer.size.x, printer.size.y - 3 - cmdline_height))
                .focused(true);
            screen.view.draw(printer);
        }

        self.statusbar
            .draw(&printer.offset((0, printer.size.y - 2 - cmdline_height)));

        if cmdline_visible {
            let printer = &printer.offset((0, printer.size.y - 1));
            self.cmdline.draw(&printer);
        }
    }

    fn layout(&mut self, size: Vec2) {
        self.last_size = size;

        self.statusbar.layout(Vec2::new(size.x, 2));
        self.cmdline.layout(Vec2::new(size.x, 1));

        if let Some(screen) = self.get_current_screen_mut() {
            screen.view.layout(Vec2::new(size.x, size.y - 3));
        }

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
            let cmdline_visible = self.cmdline.get_content().len() > 0;
            let cmdline_height = if cmdline_visible { 1 } else { 0 };

            if position.y < self.last_size.y.saturating_sub(2 + cmdline_height) {
                if let Some(ref id) = self.focus {
                    let screen = self.views.get_mut(id).unwrap();
                    screen.view.on_event(event.relativized(Vec2::new(0, 1)));
                }
            } else if position.y < self.last_size.y - cmdline_height {
                self.statusbar.on_event(
                    event.relativized(Vec2::new(0, self.last_size.y - 2 - cmdline_height)),
                );
            }

            return EventResult::Consumed(None);
        }

        if self.cmdline_focus {
            return self.cmdline.on_event(event);
        }

        if let Some(screen) = self.get_current_screen_mut() {
            screen.view.on_event(event)
        } else {
            EventResult::Ignored
        }
    }

    fn call_on_any<'a>(&mut self, s: &Selector, c: AnyCb<'a>) {
        if let Some(screen) = self.get_current_screen_mut() {
            screen.view.call_on_any(s, c);
        }
    }

    fn take_focus(&mut self, source: Direction) -> bool {
        if self.cmdline_focus {
            return self.cmdline.take_focus(source);
        }

        if let Some(screen) = self.get_current_screen_mut() {
            screen.view.take_focus(source)
        } else {
            false
        }
    }
}
