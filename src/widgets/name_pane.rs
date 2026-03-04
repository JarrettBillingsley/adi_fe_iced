
use std::rc::{ Rc };

use iced::{
	Font, Length, Padding,
	widget::{
		column, scrollable, button,
		pane_grid::{
			Content as PaneContent,
			TitleBar as PaneTitleBar,
		},

		text,
	},
};

use crate::{ Message, CONSOLAS_FONT_BOLD, FontEx };
use crate::ui::{ NameListData };
use crate::backend::{ Backend };
use crate::widgets::code_view::{ CodeViewMessage };

// ------------------------------------------------------------------------------------------------
// NamePane
// ------------------------------------------------------------------------------------------------

pub(crate) struct NamePane {
	names: Vec<NameListData>,
}

impl NamePane {
	pub(crate) fn new(backend: Rc<Backend>) -> Self {
		// TODO: keep the backend and dynamically get names... need some kind of listener in adi
		// to listen for name changes to do that
		Self { names: backend.get_all_names() }
	}

	pub(crate) fn view(&self) -> PaneContent<'_, Message> {
		let ui = scrollable(column(self.names.iter().map(|NameListData { ea, name }| {
			button(text(name).font(CONSOLAS_FONT_BOLD))
				.style(button::text)
				.on_press(CodeViewMessage::JumpTo { ea: *ea }.into())
				.into()
		})).width(Length::Fill).padding(Padding::from([0, 10])));

		let title = text("Names").size(20).font(Font::DEFAULT.bold());
		PaneContent::new(ui)
			.title_bar(PaneTitleBar::new(title).padding(10))
	}
}