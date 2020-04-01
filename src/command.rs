#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Command {
    Quit,
    NavigateBack,
    OpenLink,
    AddBookmark,
    OpenImage,
    ReloadCurrentPage,
    SavePageAs,
    GoToTop,
    GoToBottom,
    GoDown(usize),
    GoUp(usize),
    GoToNextLink,
    GoToPreviousLink
}

pub struct CommandHandler {
}

impl CommandHandler {
    pub fn new() -> Self {
        CommandHandler { }
    }

    pub fn parse(input &str) -> Command {
        Command::GoToPreviousLink
    }
}
