use crate::ncgopher::NcGopher;
use cursive::theme::ColorStyle;
use cursive::traits::View;
use cursive::vec::Vec2;
use cursive::Printer;
use std::sync::Arc;

pub struct StatusBar {
    last_size: Vec2,
    ui: Arc<NcGopher>,
}

impl StatusBar {
    pub fn new(ui: Arc<NcGopher>) -> StatusBar {
        StatusBar {
            last_size: Vec2::new(0, 0),
            ui,
        }
    }
}

impl View for StatusBar {
    fn draw(&self, printer: &Printer<'_, '_>) {
        if printer.size.x == 0 {
            warn!("status bar height is zero");
            return;
        }
        let msg = self.ui.get_message();
        printer.with_color(ColorStyle::highlight_inactive(), |printer| {
            // clear line
            printer.print_hline((0, 0), printer.size.x, " ");
            // write content
            printer.print((1, 0), msg.as_str());
        });
        printer.with_color(ColorStyle::tertiary(), |printer|{
            // clear line
            printer.print_hline((0, 1), printer.size.x, " ");
            // write content
            printer.print(
                (1, 1),
                "Commands: Use the arrow keys to move. 'b' for back, 'g' for open URL, 'ESC' for menu"
            );
        });
    }

    fn layout(&mut self, size: Vec2) {
        self.last_size = size;
    }

    fn required_size(&mut self, constraint: Vec2) -> Vec2 {
        Vec2::new(constraint.x, 2)
    }
}
