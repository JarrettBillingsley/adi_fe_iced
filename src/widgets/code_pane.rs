use std::rc::{ Rc };

use iced::{
	Font, Length, Padding, color,
	widget::{
		container, pick_list,

		pane_grid,
		pane_grid::{
			Content as PaneContent,
			TitleBar as PaneTitleBar,
		},

		text,
	},
};

use adi::{ EA, SegId };

use crate::{ Message, FontEx };
use crate::backend::{ Backend, SegmentChangedEvent };
use crate::widgets::code_view::{ CodeView, CodeViewMessage };

// ------------------------------------------------------------------------------------------------
// CodePane
// ------------------------------------------------------------------------------------------------

pub(crate) struct CodePane {
	codeview: CodeView,
	backend:  Rc<Backend>,
}

impl CodePane {
	// TODO: generate unique ID instead (could have multiple code panes open at once)
	pub(crate) const CODEVIEW_ID: &str = "panes.code.codeview";

	pub(crate) fn new(id: SegId, backend: Rc<Backend>) -> Self {
		Self {
			codeview: CodeView::new(id, backend.clone()),
			backend,
		}
	}

	pub(crate) fn set_segment(&mut self, segid: SegId) {
		if self.codeview.segid() != segid {
			self.codeview = CodeView::new(segid, self.backend.clone());
		}
	}

	pub(crate) fn dispatch_event(&self, ea: EA, ev: SegmentChangedEvent) {
		self.codeview.dispatch_event(ea, ev);
	}

	pub(crate) fn view(&self) -> PaneContent<'_, Message> {
		let list = self.codeview.view(Self::CODEVIEW_ID).map(Message::CodeView);

		let ui = container(list)
		.width(Length::Fill)
		.height(Length::Fill)
		.padding(Padding::from([0, 10]))
		.style(move |_theme| {
			container::Style::default().background(color!(0x101010))
		});

		let mut all_segs = self.backend.get_all_segments();
		all_segs.sort_by_key(|data| data.segid);
		// SAFETY: self.codeview could only have been made from a valid segment ID
		let this_seg = all_segs.iter()
			.find(|data| data.segid == self.codeview.segid()).unwrap().clone();

		let seg_selector = pick_list(
			all_segs,
			Some(this_seg),
			|segdata| CodeViewMessage::SwitchSegment { id: segdata.segid }.into());

		PaneContent::new(ui)
		.title_bar(
			PaneTitleBar::new(text("Code").size(20).font(Font::DEFAULT.bold()))
				.padding(10)
				.controls(pane_grid::Controls::new(seg_selector))
				.always_show_controls()
		)
	}
}