use cursive::traits::View;
use cursive::views::NamedView;


pub trait ViewExt: View {
    fn title(&self) -> String {
        "".into()
    }
}

impl<V: ViewExt> ViewExt for NamedView<V> {
}


pub trait IntoBoxedViewExt {
    fn as_boxed_view_ext(self) -> Box<dyn ViewExt>;
}

impl<V: ViewExt> IntoBoxedViewExt for V {
    fn as_boxed_view_ext(self) -> Box<dyn ViewExt> {
        Box::new(self)
    }
}
