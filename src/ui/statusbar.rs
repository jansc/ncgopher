use cursive::theme::{ColorStyle};
use cursive::traits::View;
use cursive::vec::Vec2;
use cursive::Printer;
use std::sync::Arc;
use crate::ncgopher::NcGopher;

pub struct StatusBar {
    last_size: Vec2,
    ui: Arc<NcGopher>,
}

impl StatusBar {
    pub fn new(ui: Arc<NcGopher>) -> StatusBar {
        StatusBar {
            last_size: Vec2::new(0, 0),
            ui
        }
    }
}

impl View for StatusBar {
    fn draw(&self, printer: &Printer<'_, '_>) {
        if printer.size.x == 0 {
            return;
        }
        let msg = self.ui.get_message();
        let style = ColorStyle::new(
            //ColorType::Color(*printer.theme.palette.xxx.unwrap()),
            //ColorType::Color(*printer.theme.palette.xxxbg.unwrap())
            ColorStyle::highlight().front,
            ColorStyle::highlight().back
        );
        printer.with_color(style, |printer| {
            printer.print(
                (0, 0),
                &vec![' '; printer.size.x].into_iter().collect::<String>()
            )
        });
        printer.print(
            (0, 1),
            &vec![' '; printer.size.x].into_iter().collect::<String>()
        );
        printer.print((1, 1), "Commands: Use the arrow keys to move. 'b' for back, 'g' for open URL, 'ESC' for menu");
        printer.with_color(style, |printer| {
            printer.print((1, 0), msg.as_str());
        });
                           
    }

    fn layout(&mut self, size: Vec2) {
        self.last_size = size;
    }

    fn required_size(&mut self, constraint: Vec2) -> Vec2 {
        Vec2::new(constraint.x, 2)
    }
}
