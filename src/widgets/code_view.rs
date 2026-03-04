use std::cell::{ RefCell };
use std::rc::{ Rc };

use iced::{ Element, widget::Column, };

use adi::{ EA, SegId };

use crate::backend::{ Backend, SegmentChangedEvent };
use crate::ui::*;
use crate::widgets::sparse_list::{ sparse_list, IContent, Change as ListChange };
use crate::widgets::code_line::{ CodeLine };

// ------------------------------------------------------------------------------------------------
// OperandLocation
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OperandLocation {
	pub(crate) bb_ea: EA,
	pub(crate) instn: usize,
	pub(crate) opn: u8,
}

// ------------------------------------------------------------------------------------------------
// CodeViewMessage
// ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) enum CodeViewMessage {
	OperandHovered { loc: OperandLocation, over: bool },
	OperandClicked { loc: OperandLocation },
	JumpTo { ea: EA },
	SwitchSegment { id: SegId },
	JumpToTop,
	JumpToBottom,
	Scroll { up: bool },
}

// ------------------------------------------------------------------------------------------------
// CodeViewRenderer
// ------------------------------------------------------------------------------------------------

struct CodeViewRenderer {
	lines: Vec<CodeLine<'static>>,
}

impl CodeViewRenderer {
	// --------------------------------------------------------------------------------------------
	// Lifecycle

	fn new() -> Self {
		Self { lines: vec![] }
	}

	fn finish(self) -> Vec<CodeLine<'static>> {
		self.lines
	}

	// --------------------------------------------------------------------------------------------
	// Rendering methods

	fn blank_line(&mut self, ea: EA) {
		self.lines.push(CodeLine::new_blank(ea));
	}

	fn error_line(&mut self, ea: EA, text_ea: TextEA, message: impl Into<String>) {
		self.lines.push(CodeLine::new_error(ea, text_ea, message.into()));
	}

	fn comment_line(&mut self, ea: EA, comment: impl Into<String>) {
		self.lines.push(CodeLine::new_comment(ea, comment.into()));
	}

	fn label_line(&mut self, ea: EA, label: String) {
		if !label.is_empty() {
			let label = format!("                   {}", label);
			self.lines.push(CodeLine::new_label(ea, label));
		}
	}

	fn code_line(&mut self, ea: EA, text_ea: TextEA, bb_ea: EA, instn: usize, code_bytes: String,
	mnemonic: String, operands: Vec<CodeOpData>) {
		self.lines.push(
			CodeLine::new_code(ea, text_ea, bb_ea, instn, code_bytes, mnemonic, operands));
	}

	fn unknown_line(&mut self, ea: EA, text_ea: TextEA, bytes: String) {
		self.lines.push(CodeLine::new_unknown(ea, text_ea, bytes));
	}

	fn func_data(&mut self, ea: EA, data: Option<FuncData>) {
		let Some(data) = data else { return; };

		self.comment_line(ea,
			"------------------------------------------------------------------------------");

		match data.kind {
			FuncDataKind::Piece => {
				self.comment_line(ea, format!("(Piece of function {})", data.name));
			}
			FuncDataKind::Head { attrs, entrypoints } => {
				self.comment_line(ea, format!("Function {}", data.name));

				if let Some(attrs) = attrs {
					self.comment_line(ea, format!("Attributes: {}", attrs));
				}

				if let Some(entrypoints) = entrypoints {
					self.comment_line(ea, format!("Entry points: {}", entrypoints));
				}
			}
		}
	}
}

// ------------------------------------------------------------------------------------------------
// Rendering methods for various pieces of code
// ------------------------------------------------------------------------------------------------

impl BasicBlockData {
	fn render(self) -> Vec<CodeLine<'static>> {
		let mut r = CodeViewRenderer::new();

		// TODO: inrefs
		// TODO: MMU state

		// SAFETY: lines is never empty
		let first_ea = &self.lines.first().unwrap().ea;
		let last_ea = self.lines.last().unwrap().ea.clone();
		r.func_data(first_ea.clone(), self.func);
		r.label_line(first_ea.clone(), self.label);

		for (instn, line) in self.lines.into_iter().enumerate() {
			r.code_line(line.ea, line.text_ea, self.ea, instn, line.bytes, line.mnemonic,
				line.operands);
			// TODO: outrefs
		}

		r.blank_line(last_ea);
		r.finish()
	}
}

impl UnknownData {
	fn render(self) -> Vec<CodeLine<'static>> {
		let mut r = CodeViewRenderer::new();

		// SAFETY: lines is never empty
		let last_ea = self.lines.last().unwrap().ea.clone();

		for line in self.lines.into_iter() {
			r.unknown_line(line.ea, line.text_ea, line.bytes);
		}

		r.blank_line(last_ea);
		r.finish()
	}
}

impl CodeViewItem {
	fn render(self) -> Element<'static, CodeViewMessage> {
		let lines = match self {
			CodeViewItem::BasicBlock(bb) => bb.render(),
			CodeViewItem::DataItem(ea, text_ea) => {
				// TODO: data rendering
				let mut r = CodeViewRenderer::new();
				r.error_line(ea, text_ea, "DATA UNIMPLEMENTED");
				r.finish()
			}
			CodeViewItem::Unknown(unk) => unk.render(),
		};

		Column::with_children(lines.into_iter().map(|line| line.into())).into()
	}
}

// ------------------------------------------------------------------------------------------------
// CodeView
// ------------------------------------------------------------------------------------------------

pub(crate) struct CodeView {
	backend: Rc<Backend>,
	id:      SegId,
	changes: RefCell<Vec<ListChange>>,
}

impl CodeView {
	pub(crate) fn new(id: SegId, backend: Rc<Backend>) -> Self {
		Self {
			backend,
			id,
			changes: RefCell::new(Vec::new()),
		}
	}

	fn render_span(&self, ea: EA) -> CodeViewItem {
		self.backend.get_rendered_span(ea)
	}

	pub(crate) fn segid(&self) -> SegId {
		self.id
	}

	fn ea_after(&self, ea: EA) -> Option<EA> {
		self.backend.get_span_after(ea).map(|span| span.start())
	}

	fn ea_before(&self, ea: EA) -> Option<EA> {
		self.backend.get_span_before(ea).map(|span| span.start())
	}

	pub(crate) fn dispatch_event(&self, ea: EA, ev: SegmentChangedEvent) {
		if ea.seg() == self.id {
			use SegmentChangedEvent::*;
			self.changes.borrow_mut().push(
				match ev {
					Add    => ListChange::Added   { idx: ea.offs() },
					Remove => ListChange::Removed { idx: ea.offs() },
					Change => ListChange::Changed { idx: ea.offs() },
				}
			);
		}
	}

	pub(crate) fn view(&self, id: &'static str) -> Element<'_, CodeViewMessage> {
		sparse_list(self, |_, ea: EA| self.render_span(ea).render())
			.id(id)
			.into()
	}
}

// just to keep my thoughts straight
type SegOffs = usize;

impl<'a> IContent<'a, EA> for CodeView {
	fn len(&self) -> usize {
		self.backend.get_num_spans(self.id)
	}

	fn first_index(&self) -> Option<SegOffs> {
		// by definition
		Some(0)
	}

	fn last_index(&self) -> Option<SegOffs> {
		Some(self.backend.get_last_span_offset(self.id))
	}

	fn get(&self, idx: SegOffs) -> Option<EA> {
		Some(self.backend.get_span(EA::new(self.id, idx)).start())
	}

	fn items_before(&'a self, idx: SegOffs)
	-> Box<dyn Iterator<Item = (SegOffs, EA)> + 'a> {
		Box::new(SpansBefore { seg: self, ea: EA::new(self.id, idx) })
	}

	fn items_after(&'a self, idx: SegOffs)
	-> Box<dyn Iterator<Item = (SegOffs, EA)> + 'a> {
		Box::new(SpansAfter { seg: self, ea: EA::new(self.id, idx) })
	}

	fn changes(&self) -> Vec<ListChange> {
		self.changes.take()
	}
}

struct SpansAfter<'a> {
	seg: &'a CodeView,
	ea:  EA,
}

impl<'a> Iterator for SpansAfter<'a> {
	type Item = (SegOffs, EA);

	fn next(&mut self) -> Option<Self::Item> {
		self.seg.ea_after(self.ea).map(|next_ea| {
			self.ea = next_ea;
			(next_ea.offs(), next_ea)
		})
	}
}

struct SpansBefore<'a> {
	seg: &'a CodeView,
	ea:  EA,
}

impl<'a> Iterator for SpansBefore<'a> {
	type Item = (SegOffs, EA);

	fn next(&mut self) -> Option<Self::Item> {
		self.seg.ea_before(self.ea).map(|next_ea| {
			self.ea = next_ea;
			(next_ea.offs(), next_ea)
		})
	}
}
